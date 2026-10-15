using System;
using System.Collections.Generic;
using System.Threading.Tasks;
using Taba;

// Creates a queue, publishes three orders, consumes with long polling, acknowledges each one and deletes the queue.
//
//   TABA_URL    client API address (default http://localhost:8080, the Compose HAProxy)
//   TABA_TOKEN  token with publish, consume, create and admin (the root token works)
//
// The Compose root token:
//   docker compose run --rm --entrypoint cat taba-cli /secrets/root-token
internal static class Program
{
    private static async Task<int> Main(string[] args)
    {
        string url = Environment.GetEnvironmentVariable("TABA_URL") ?? "http://localhost:8080";
        string token = Environment.GetEnvironmentVariable("TABA_TOKEN");
        if (string.IsNullOrWhiteSpace(token))
        {
            Console.Error.WriteLine("Set TABA_TOKEN (with Compose: docker compose run --rm --entrypoint cat taba-cli /secrets/root-token).");
            return 2;
        }
        string topic = args.Length > 0 ? args[0] : "console-example";

        using (var client = new TabaClient(new TabaConfig(url) { Token = token, UserAgent = "taba-console-example/1.0" }))
        {
            try
            {
                Topic created = await client.CreateTopicAsync(new TopicRequest { Name = topic, Type = "queue" });
                Console.WriteLine($"Queue '{created.Name}' created (id {created.Id}).");

                for (int order = 1; order <= 3; order++)
                {
                    PublishResponse published = await client.PublishRawJsonAsync(topic, $"{{\"order\": {order}, \"amount\": {order * 10}.50}}");
                    Console.WriteLine($"Published order {order}: message {published.Id}.");
                }

                int received = 0;
                for (int round = 0; round < 10 && received < 3; round++)
                {
                    // Long polling: waits up to 5 s for messages; each one stays invisible for 30 s.
                    IList<Message> messages = await client.ConsumeAsync(topic, new ConsumeOptions { MaxMessages = 5, VisibilityTimeout = 30, WaitTime = 5 });
                    foreach (Message message in messages)
                    {
                        Console.WriteLine($"Received {message.BodyText} (delivery {message.DeliveryCount}).");
                        await client.AckAsync(topic, message);
                        received++;
                    }
                }
                Console.WriteLine($"{received} messages received and acknowledged.");
                return received == 3 ? 0 : 1;
            }
            catch (TabaException e)
            {
                // Test the Code, never the message.
                Console.Error.WriteLine($"Error {e.Status} {e.Code}: {e.Message} (requestId {e.RequestId})");
                return 1;
            }
            finally
            {
                try
                {
                    await client.DeleteTopicAsync(topic);
                    Console.WriteLine($"Queue '{topic}' deleted.");
                }
                catch (TabaException)
                {
                }
            }
        }
    }
}
