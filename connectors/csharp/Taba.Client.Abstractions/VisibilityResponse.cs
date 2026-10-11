namespace Taba
{
    /// <summary>A resposta de <c>changeVisibility</c>.</summary>
    public sealed class VisibilityResponse
    {
        internal VisibilityResponse()
        {
        }

        public string Id { get; internal set; }

        public string InvisibleUntil { get; internal set; }

        public string Raw { get; internal set; }
    }
}
