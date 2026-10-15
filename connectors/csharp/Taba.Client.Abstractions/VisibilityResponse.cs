namespace Taba
{
    /// <summary>The <c>changeVisibility</c> response.</summary>
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
