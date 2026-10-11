using System.Text;

namespace Taba
{
    internal static class Utf8
    {
        /// <summary>UTF-8 sem marca de ordem de bytes: o texto vai e volta sempre assim, qualquer que seja o charset local.</summary>
        public static readonly Encoding Encoding = new UTF8Encoding(false);
    }
}
