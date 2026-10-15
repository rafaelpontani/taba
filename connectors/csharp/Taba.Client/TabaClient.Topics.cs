using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;

namespace Taba
{
    public sealed partial class TabaClient
    {
        // -----------------------------------------------------------------------------------------
        // The nine operations
        // -----------------------------------------------------------------------------------------

        /// <summary><c>POST /topics</c>.</summary>
        public async Task<Topic> CreateTopicAsync(TopicRequest request, CancellationToken cancellationToken = default(CancellationToken))
        {
            if (request == null)
            {
                throw new ArgumentNullException("request");
            }
            var call = new ApiCall(HttpMethod.Post, "/topics").Json(RequestJson.Create(request));
            call.Recover.Add(new KeyValuePair<int, string>(409, "topic.already-exists"));
            ApiReply reply = await SendAsync(call, cancellationToken).ConfigureAwait(false);
            if (reply.Recovered)
            {
                return await GetTopicAsync(request.Name, cancellationToken).ConfigureAwait(false);
            }
            return ModelParser.ParseTopic(reply.Text);
        }

        /// <summary><c>GET /topics/{topic}</c>.</summary>
        public async Task<Topic> GetTopicAsync(string name, CancellationToken cancellationToken = default(CancellationToken))
        {
            ApiReply reply = await SendAsync(new ApiCall(HttpMethod.Get, "/topics/" + Segment(name)), cancellationToken)
                .ConfigureAwait(false);
            return ModelParser.ParseTopic(reply.Text);
        }

        /// <summary><c>GET /topics</c>. <c>null</c> parameters do not go in the query.</summary>
        public async Task<Page<Topic>> ListTopicsAsync(
            int? limit = null,
            string cursor = null,
            string name = null,
            CancellationToken cancellationToken = default(CancellationToken))
        {
            var call = new ApiCall(HttpMethod.Get, "/topics");
            if (limit.HasValue)
            {
                call.With("limit", limit.Value);
            }
            if (cursor != null)
            {
                call.With("cursor", cursor);
            }
            if (name != null)
            {
                call.With("name", name);
            }
            ApiReply reply = await SendAsync(call, cancellationToken).ConfigureAwait(false);
            JsonFields fields = ModelParser.Fields(reply.Text, "page");
            var items = new List<Topic>();
            try
            {
                foreach (string item in Json.ArrayItems(fields.Raw("items") ?? "[]"))
                {
                    items.Add(ModelParser.ParseTopic(item));
                }
            }
            catch (JsonException e)
            {
                throw TabaException.Client("client.invalid-response", "page items: " + e.Message);
            }
            return new Page<Topic>(items, fields.String("nextCursor"), reply.Text);
        }

        /// <summary>All topics, page by page.</summary>
        public async Task<IList<Topic>> ListAllTopicsAsync(string name = null, CancellationToken cancellationToken = default(CancellationToken))
        {
            var all = new List<Topic>();
            string cursor = null;
            do
            {
                Page<Topic> page = await ListTopicsAsync(null, cursor, name, cancellationToken).ConfigureAwait(false);
                all.AddRange(page.Items);
                cursor = page.NextCursor;
            }
            while (cursor != null);
            return all;
        }

        /// <summary><c>PATCH /topics/{topic}</c>. It is a real <c>PATCH</c>, with no verb-override header.</summary>
        public async Task<Topic> UpdateTopicAsync(string name, TopicUpdate update, CancellationToken cancellationToken = default(CancellationToken))
        {
            if (update == null)
            {
                throw new ArgumentNullException("update");
            }
            var call = new ApiCall(Patch, "/topics/" + Segment(name)).Json(RequestJson.Update(update));
            ApiReply reply = await SendAsync(call, cancellationToken).ConfigureAwait(false);
            return ModelParser.ParseTopic(reply.Text);
        }

        /// <summary><c>DELETE /topics/{topic}</c>.</summary>
        public async Task DeleteTopicAsync(string name, CancellationToken cancellationToken = default(CancellationToken))
        {
            var call = new ApiCall(HttpMethod.Delete, "/topics/" + Segment(name));
            call.Recover.Add(new KeyValuePair<int, string>(404, "topic.not-found"));
            await SendAsync(call, cancellationToken).ConfigureAwait(false);
        }


    }
}
