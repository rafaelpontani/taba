using System;
using System.Collections.Generic;
using System.Threading.Tasks;
using Taba;

// Cria uma fila, publica três pedidos, consome com long polling, confirma cada um e apaga a fila.
//
//   TABA_URL    endereço da API de cliente (padrão http://localhost:8080, o HAProxy do Compose)
//   TABA_TOKEN  token com publish, consume, create e admin (o token raiz serve)
//
// O token raiz do Compose:
//   docker compose run --rm --entrypoint cat taba-cli /secrets/root-token
internal static class Program
{
    private static async Task<int> Main(string[] args)
    {
        string url = Environment.GetEnvironmentVariable("TABA_URL") ?? "http://localhost:8080";
        string token = Environment.GetEnvironmentVariable("TABA_TOKEN");
        if (string.IsNullOrWhiteSpace(token))
        {
            Console.Error.WriteLine("Defina TABA_TOKEN (com o Compose: docker compose run --rm --entrypoint cat taba-cli /secrets/root-token).");
            return 2;
        }
        string topic = args.Length > 0 ? args[0] : "exemplo-console";

        using (var client = new TabaClient(new TabaConfig(url) { Token = token, UserAgent = "taba-exemplo-console/1.0" }))
        {
            try
            {
                Topic created = await client.CreateTopicAsync(new TopicRequest { Name = topic, Type = "queue" });
                Console.WriteLine($"Fila '{created.Name}' criada (id {created.Id}).");

                for (int pedido = 1; pedido <= 3; pedido++)
                {
                    PublishResponse published = await client.PublishJsonAsync(topic, $"{{\"pedido\": {pedido}, \"valor\": {pedido * 10}.50}}");
                    Console.WriteLine($"Publicado o pedido {pedido}: mensagem {published.Id}.");
                }

                int received = 0;
                for (int round = 0; round < 10 && received < 3; round++)
                {
                    // Long polling: espera até 5 s por mensagens; cada uma fica invisível por 30 s.
                    IList<Message> messages = await client.ConsumeAsync(topic, new ConsumeOptions { MaxMessages = 5, VisibilityTimeout = 30, WaitTime = 5 });
                    foreach (Message message in messages)
                    {
                        Console.WriteLine($"Recebido {message.BodyText} (entrega {message.DeliveryCount}).");
                        await client.AckAsync(topic, message);
                        received++;
                    }
                }
                Console.WriteLine($"{received} mensagens recebidas e confirmadas.");
                return received == 3 ? 0 : 1;
            }
            catch (TabaException e)
            {
                // Teste o Code, nunca a mensagem.
                Console.Error.WriteLine($"Erro {e.Status} {e.Code}: {e.Message} (requestId {e.RequestId})");
                return 1;
            }
            finally
            {
                try
                {
                    await client.DeleteTopicAsync(topic);
                    Console.WriteLine($"Fila '{topic}' apagada.");
                }
                catch (TabaException)
                {
                }
            }
        }
    }
}
