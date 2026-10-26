"""Keep each (provider, model) under its requests-per-minute budget so free tiers don't throttle the run."""

from __future__ import annotations

import time
from collections.abc import Callable


class RateLimiter:
    def __init__(self, clock: Callable[[], float] = time.monotonic,
                 sleep: Callable[[float], None] = time.sleep) -> None:
        self._next: dict[str, float] = {}
        self._clock = clock
        self._sleep = sleep

    def wait(self, key: str, rpm: float) -> None:
        """Block until `key` may send its next request (requests spaced 60/rpm seconds apart)."""
        now = self._clock()
        ready = self._next.get(key, now)
        if ready > now:
            self._sleep(ready - now)
            now = ready
        self._next[key] = now + 60.0 / rpm

    def back_off(self, key: str, seconds: float) -> None:
        """After a 429: push this key's next slot out."""
        self._next[key] = max(self._next.get(key, 0.0), self._clock() + seconds)
