"""
The Python client of Taba (https://github.com/rafaelpontani/taba): the nine operations of the client API,
`subscribe`, consuming with a handler and the call retries of the common contract. No dependencies.

`taba.Client` is synchronous; `taba.aio.AsyncClient` is the `asyncio` one.
"""

from ._version import CONTRACT_VERSION, VERSION
from .cancel import Cancelled, CancelToken
from .client import Client
from .errors import TabaError
from .models import (
    UNSET,
    BatchMessage,
    BatchResult,
    Config,
    ConsumeOptions,
    DistributorDelivery,
    Message,
    MessageContext,
    Page,
    PublishResponse,
    Topic,
    TopicRequest,
    TopicUpdate,
    VisibilityResponse,
)

__version__ = VERSION

__all__ = [
    "CONTRACT_VERSION",
    "UNSET",
    "VERSION",
    "BatchMessage",
    "BatchResult",
    "Cancelled",
    "CancelToken",
    "Client",
    "Config",
    "ConsumeOptions",
    "DistributorDelivery",
    "Message",
    "MessageContext",
    "Page",
    "PublishResponse",
    "TabaError",
    "Topic",
    "TopicRequest",
    "TopicUpdate",
    "VisibilityResponse",
]
