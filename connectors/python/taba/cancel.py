"""Cancellation of a call or of a loop in the synchronous client (the `CancellationToken` of the C# connector)."""

import threading
from typing import Callable, List


class Cancelled(Exception):
    """The call was cancelled through a `CancelToken`. It is not a `TabaError`: it is not a failure of the call."""


class CancelToken:
    """
    Cancel a `Client` call or a `subscribe` loop from another thread.

    >>> token = CancelToken()
    >>> threading.Timer(60, token.cancel).start()
    >>> client.subscribe("orders", handler, cancel=token)   # returns when cancelled

    Cancelling closes the connection of the call in flight, so a long polling does not wait for its timeout.
    """

    def __init__(self) -> None:
        self._event = threading.Event()
        self._lock = threading.Lock()
        self._callbacks: List[Callable[[], None]] = []

    def cancel(self) -> None:
        with self._lock:
            self._event.set()
            callbacks = list(self._callbacks)
        for callback in callbacks:
            try:
                callback()
            except Exception:  # closing a connection that is already closed
                pass

    def is_set(self) -> bool:
        return self._event.is_set()

    @property
    def cancelled(self) -> bool:
        return self._event.is_set()

    def wait(self, timeout: float) -> bool:
        """Waits up to `timeout` seconds; `True` when cancelled."""
        return self._event.wait(timeout)

    def raise_if_cancelled(self) -> None:
        if self._event.is_set():
            raise Cancelled()

    def _attach(self, callback: Callable[[], None]) -> None:
        with self._lock:
            self._callbacks.append(callback)
            already = self._event.is_set()
        if already:
            callback()

    def _detach(self, callback: Callable[[], None]) -> None:
        with self._lock:
            if callback in self._callbacks:
                self._callbacks.remove(callback)
