//! The S3 (and MinIO) client over the official `aws-sdk-s3` (D-S3: Q11). Without credentials in the
//! configuration, the SDK uses the default AWS chain (environment variables, profile, task
//! role, instance IAM).

use aws_sdk_s3::Client;
use aws_sdk_s3::config::retry::RetryConfig;
use aws_sdk_s3::config::timeout::TimeoutConfig;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_s3::primitives::ByteStream;

use super::{Object, ObjectInfo, ObjectStore, StoreError, StoreFuture};

/// What `backup.s3.*` configures.
#[derive(Debug, Clone, Default)]
pub struct S3Settings {
    pub bucket: String,
    pub region: Option<String>,
    /// Outside AWS (MinIO, Ceph): the service address.
    pub endpoint: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    /// `bucket` in the path (`http://host/bucket/key`) instead of in the host name. Required on
    /// MinIO.
    pub path_style: bool,
}

pub struct S3Store {
    client: Client,
    bucket: String,
}

/// The HTTP code of an SDK failure, if the response arrived.
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
        // Short deadlines: an S3 that does not answer cannot hold the upload or the drain for minutes
        // (the backup tries again on the next pass; whoever waits has the `backup.drain.timeout` deadline).
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
            // Outside AWS the region does not matter, but the SDK requires one.
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
        // Only computes and checks checksums when the service requires it: MinIO and other older compatible
        // S3s do not know them, and the ETag already covers the integrity the backup uses.
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
                // A range that starts after the end (empty object): nothing to read.
                Err(e) if status(&e) == Some(416) => Ok(Some(Vec::new())),
                Err(e) => Err(unavailable(&e)),
            }
        })
    }
}

/// The result of a conditional write: `412` and `409` say the condition failed; `501` and
/// `400` with the conditional header, that the store does not know it.
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
