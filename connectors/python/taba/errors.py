"""The only kind of error of the connector (doc 21, "The common contract")."""

from typing import Optional


class TabaError(Exception):
    """
    The API `ErrorResponse`, or a failure of the client itself (`status` 0 and `code` `client.network`,
    `client.timeout`, `client.invalid-config`, `client.invalid-response` or `client.invalid-request`).
    Test the `code`, never the `message`.
    """

    def __init__(
        self,
        status: int,
        code: str,
        message: str,
        request_id: Optional[str] = None,
        retry_after: Optional[int] = None,
        details: Optional[str] = None,
        body: bytes = b"",
    ) -> None:
        super().__init__(message)
        #: The HTTP status, or zero on a failure of the client itself.
        self.status = status
        #: The `group.reason` code of the error.
        self.code = code or ""
        self.message = message
        #: The `requestId` of the `ErrorResponse`, or the `X-Request-Id` header.
        self.request_id = request_id
        #: The seconds of the `Retry-After`, if the response carried it.
        self.retry_after = retry_after
        #: The `details` of the `ErrorResponse`, as JSON text.
        self.details = details
        #: The response body, as it came (empty on a client failure).
        self.body = body

    @property
    def is_client_side(self) -> bool:
        """A failure of the client itself, not a server response."""
        return self.status == 0

    def __str__(self) -> str:
        suffix = "" if self.request_id is None else " (requestId %s)" % self.request_id
        return "%s: %s%s" % (self.code, self.message, suffix)

    @classmethod
    def client(cls, code: str, message: str) -> "TabaError":
        return cls(0, code, message)
