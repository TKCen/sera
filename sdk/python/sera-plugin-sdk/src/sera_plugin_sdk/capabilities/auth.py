"""AuthProvider capability ABC."""

from __future__ import annotations

import abc

from ..types import AuthRequest, AuthResponse, AuthzRequest, AuthzResponse
from .base import Capability


class AuthProvider(Capability, abc.ABC):
    @abc.abstractmethod
    async def authenticate(self, req: AuthRequest) -> AuthResponse: ...

    @abc.abstractmethod
    async def authorize(self, req: AuthzRequest) -> AuthzResponse: ...


__all__ = ["AuthProvider"]
