"""Keep each (provider, model) under its requests-per-minute budget so free tiers don't throttle the run.

Thread-safe: each caller reserves the next free slot under a lock, then sleeps outside it.
"""

from __future__ import annotations

import threading
import time
from collections.abc import Callable


class RateLimiter:
    def __init__(self, clock: Callable[[], float] = time.monotonic,
                 sleep: Callable[[float], None] = time.sleep) -> None:
        self._next: dict[str, float] = {}
        self._clock = clock
        self._sleep = sleep
        self._lock = threading.Lock()

    def wait(self, key: str, rpm: float) -> None:
        """Block until `key` may send its next request (requests spaced 60/rpm seconds apart)."""
        with self._lock:
            now = self._clock()
            slot = max(self._next.get(key, now), now)
            self._next[key] = slot + 60.0 / rpm
        if slot > now:
            self._sleep(slot - now)

    def back_off(self, key: str, seconds: float) -> None:
        """After a 429: push this key's next slot out."""
        with self._lock:
            self._next[key] = max(self._next.get(key, 0.0), self._clock() + seconds)
