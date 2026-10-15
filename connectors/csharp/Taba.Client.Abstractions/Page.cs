using System.Collections.Generic;

namespace Taba
{
    /// <summary>A page (<c>Page&lt;T&gt;</c>): the items and the cursor of the next one, if any.</summary>
    public sealed class Page<T>
    {
        internal Page(IList<T> items, string nextCursor, string raw)
        {
            Items = items;
            NextCursor = nextCursor;
            Raw = raw;
        }

        public IList<T> Items { get; private set; }

        /// <summary>The cursor of the next page, or <c>null</c> on the last one.</summary>
        public string NextCursor { get; private set; }

        public string Raw { get; private set; }
    }
}
