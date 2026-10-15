using System.Text;

namespace Taba
{
    internal static class Utf8
    {
        /// <summary>UTF-8 without a byte order mark: text always goes and comes back like this, whatever the local charset.</summary>
        public static readonly Encoding Encoding = new UTF8Encoding(false);
    }
}
