using System.Collections.Generic;

namespace Taba
{
    /// <summary>Uma página (<c>Page&lt;T&gt;</c>): os itens e o cursor da próxima, se houver.</summary>
    public sealed class Page<T>
    {
        internal Page(IList<T> items, string nextCursor, string raw)
        {
            Items = items;
            NextCursor = nextCursor;
            Raw = raw;
        }

        public IList<T> Items { get; private set; }

        /// <summary>O cursor da próxima página, ou <c>null</c> na última.</summary>
        public string NextCursor { get; private set; }

        public string Raw { get; private set; }
    }
}
