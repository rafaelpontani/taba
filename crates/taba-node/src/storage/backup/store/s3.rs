//! O cliente do S3 (e do MinIO) sobre o `aws-sdk-s3` oficial (D-S3: Q11). Sem credenciais na
//! configuração, o SDK usa a cadeia padrão da AWS (variáveis de ambiente, perfil, papel da
//! tarefa, IAM da instância).

use aws_sdk_s3::Client;
use aws_sdk_s3::config::retry::RetryConfig;
use aws_sdk_s3::config::timeout::TimeoutConfig;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_s3::primitives::ByteStream;

use super::{Object, ObjectInfo, ObjectStore, StoreError, StoreFuture};

/// O que `backup.s3.*` configura.
#[derive(Debug, Clone, Default)]
pub struct S3Settings {
    pub bucket: String,
    pub region: Option<String>,
    /// Fora da AWS (MinIO, Ceph): o endereço do serviço.
    pub endpoint: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    /// `bucket` no caminho (`http://host/bucket/chave`) em vez de no nome do host. Obrigatório no
    /// MinIO.
    pub path_style: bool,
}

pub struct S3Store {
    client: Client,
    bucket: String,
}

/// O código HTTP de uma falha do SDK, se a resposta chegou.
fn status<E>(error: &SdkError<E, aws_sdk_s3::config::http::HttpResponse>) -> Option<u16> {
    error.raw_response().map(|r| r.status().as_u16())
}

fn unavailable<E>(error: &SdkError<E, aws_sdk_s3::config::http::HttpResponse>) -> StoreError
where
    E: ProvideErrorMetadata + std::error::Error + 'static,
{
    let code = error.code().unwrap_or("");
    let text = error
        .message()
        .map(str::to_owned)
        .unwrap_or_else(|| error.to_string());
    StoreError::Unavailable(format!("{code} {text}").trim().to_owned())
}

impl S3Store {
    pub async fn connect(settings: &S3Settings) -> Self {
        // Prazos curtos: um S3 que não responde não pode segurar o envio nem o drain por minutos
        // (o backup tenta de novo na passada seguinte; quem espera tem o prazo `backup.drain.timeout`).
        let mut loader = aws_config::defaults(BehaviorVersion::latest())
            .timeout_config(
                TimeoutConfig::builder()
                    .connect_timeout(std::time::Duration::from_secs(3))
                    .operation_attempt_timeout(std::time::Duration::from_secs(15))
                    .operation_timeout(std::time::Duration::from_secs(30))
                    .build(),
            )
            .retry_config(RetryConfig::standard().with_max_attempts(2));
        if let Some(region) = &settings.region {
            loader = loader.region(Region::new(region.clone()));
        } else if settings.endpoint.is_some() {
            // Fora da AWS a região não importa, mas o SDK exige uma.
            loader = loader.region(Region::new("us-east-1"));
        }
        if let (Some(access), Some(secret)) = (&settings.access_key, &settings.secret_key) {
            loader = loader.credentials_provider(Credentials::new(
                access.clone(),
                secret.clone(),
                None,
                None,
                "taba",
            ));
        }
        let shared = loader.load().await;
        let mut config = aws_sdk_s3::config::Builder::from(&shared);
        if let Some(endpoint) = &settings.endpoint {
            config = config.endpoint_url(endpoint.clone());
        }
        // Só calcula e confere somas de verificação quando o serviço exige: o MinIO e outros S3
        // compatíveis mais antigos não as conhecem, e o ETag já cobre a integridade que o backup usa.
        config = config
            .force_path_style(settings.path_style)
            .request_checksum_calculation(
                aws_sdk_s3::config::RequestChecksumCalculation::WhenRequired,
            )
            .response_checksum_validation(
                aws_sdk_s3::config::ResponseChecksumValidation::WhenRequired,
            );
        Self {
            client: Client::from_conf(config.build()),
            bucket: settings.bucket.clone(),
        }
    }
}

impl ObjectStore for S3Store {
    fn put(&self, key: &str, body: Vec<u8>) -> StoreFuture<'_, String> {
        let key = key.to_owned();
        Box::pin(async move {
            let out = self
                .client
                .put_object()
                .bucket(&self.bucket)
                .key(&key)
                .body(ByteStream::from(body))
                .send()
                .await
                .map_err(|e| unavailable(&e))?;
            Ok(out.e_tag().unwrap_or_default().to_owned())
        })
    }

    fn put_if_absent(&self, key: &str, body: Vec<u8>) -> StoreFuture<'_, bool> {
        let key = key.to_owned();
        Box::pin(async move {
            let result = self
                .client
                .put_object()
                .bucket(&self.bucket)
                .key(&key)
                .body(ByteStream::from(body))
                .if_none_match("*")
                .send()
                .await;
            conditional(result)
        })
    }

    fn put_if_match(&self, key: &str, body: Vec<u8>, etag: &str) -> StoreFuture<'_, bool> {
        let (key, etag) = (key.to_owned(), etag.to_owned());
        Box::pin(async move {
            let result = self
                .client
                .put_object()
                .bucket(&self.bucket)
                .key(&key)
                .body(ByteStream::from(body))
                .if_match(etag)
                .send()
                .await;
            conditional(result)
        })
    }

    fn get(&self, key: &str) -> StoreFuture<'_, Option<Object>> {
        let key = key.to_owned();
        Box::pin(async move {
            match self
                .client
                .get_object()
                .bucket(&self.bucket)
                .key(&key)
                .send()
                .await
            {
                Ok(out) => {
                    let etag = out.e_tag().unwrap_or_default().to_owned();
                    let body = out
                        .body
                        .collect()
                        .await
                        .map_err(|e| StoreError::Unavailable(e.to_string()))?
                        .into_bytes()
                        .to_vec();
                    Ok(Some(Object { body, etag }))
                }
                Err(e) if status(&e) == Some(404) => Ok(None),
                Err(e) => Err(unavailable(&e)),
            }
        })
    }

    fn get_range(&self, key: &str, start: u64, len: u64) -> StoreFuture<'_, Option<Vec<u8>>> {
        let range = format!("bytes={start}-{}", start + len.max(1) - 1);
        self.ranged(key, range)
    }

    fn get_suffix(&self, key: &str, len: u64) -> StoreFuture<'_, Option<Vec<u8>>> {
        self.ranged(key, format!("bytes=-{}", len.max(1)))
    }

    fn list(&self, prefix: &str) -> StoreFuture<'_, Vec<ObjectInfo>> {
        let prefix = prefix.to_owned();
        Box::pin(async move {
            let mut pages = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&prefix)
                .into_paginator()
                .send();
            let mut items = Vec::new();
            while let Some(page) = pages.next().await {
                let page = page.map_err(|e| unavailable(&e))?;
                for object in page.contents() {
                    items.push(ObjectInfo {
                        key: object.key().unwrap_or_default().to_owned(),
                        size: object.size().unwrap_or_default().max(0) as u64,
                        etag: object.e_tag().unwrap_or_default().to_owned(),
                    });
                }
            }
            Ok(items)
        })
    }

    fn delete(&self, key: &str) -> StoreFuture<'_, ()> {
        let key = key.to_owned();
        Box::pin(async move {
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(&key)
                .send()
                .await
                .map_err(|e| unavailable(&e))?;
            Ok(())
        })
    }
}

impl S3Store {
    fn ranged(&self, key: &str, range: String) -> StoreFuture<'_, Option<Vec<u8>>> {
        let key = key.to_owned();
        Box::pin(async move {
            match self
                .client
                .get_object()
                .bucket(&self.bucket)
                .key(&key)
                .range(range)
                .send()
                .await
            {
                Ok(out) => Ok(Some(
                    out.body
                        .collect()
                        .await
                        .map_err(|e| StoreError::Unavailable(e.to_string()))?
                        .into_bytes()
                        .to_vec(),
                )),
                Err(e) if status(&e) == Some(404) => Ok(None),
                // Uma faixa que começa depois do fim (objeto vazio): nada a ler.
                Err(e) if status(&e) == Some(416) => Ok(Some(Vec::new())),
                Err(e) => Err(unavailable(&e)),
            }
        })
    }
}

/// O resultado de uma escrita condicional: `412` e `409` dizem que a condição falhou; `501` e
/// `400` com o cabeçalho condicional, que o depósito não o conhece.
fn conditional<T, E>(
    result: Result<T, SdkError<E, aws_sdk_s3::config::http::HttpResponse>>,
) -> Result<bool, StoreError>
where
    E: ProvideErrorMetadata + std::error::Error + 'static,
{
    match result {
        Ok(_) => Ok(true),
        Err(e) => match status(&e) {
            Some(412 | 409) => Ok(false),
            Some(501) => Err(StoreError::Unsupported(e.to_string())),
            _ => Err(unavailable(&e)),
        },
    }
}
