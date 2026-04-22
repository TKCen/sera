"""SecretProvider capability ABC."""

from __future__ import annotations

import abc

from ..types import SecretRef, SecretValue
from .base import Capability


class SecretProvider(Capability, abc.ABC):
    @abc.abstractmethod
    async def resolve(self, ref: SecretRef) -> SecretValue: ...

    @abc.abstractmethod
    async def list_secrets(self) -> list[SecretRef]: ...


__all__ = ["SecretProvider"]
