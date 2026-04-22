"""Tests for :mod:`sera_plugin_sdk.errors`."""

from __future__ import annotations

import pytest

from sera_plugin_sdk.errors import (
    PluginCapabilityError,
    PluginDispatchError,
    PluginError,
    PluginRegistrationError,
    PluginTransportError,
)


def test_plugin_error_defaults_code_and_message() -> None:
    err = PluginError()
    assert err.code == "PLUGIN_ERROR"
    assert err.message == "PLUGIN_ERROR"
    assert str(err) == "[PLUGIN_ERROR] PLUGIN_ERROR"


def test_plugin_error_explicit_code_and_message() -> None:
    err = PluginError("MY_CODE", "something went wrong")
    assert err.code == "MY_CODE"
    assert err.message == "something went wrong"
    assert str(err) == "[MY_CODE] something went wrong"


@pytest.mark.parametrize(
    ("cls", "expected_code"),
    [
        (PluginRegistrationError, "PLUGIN_REGISTRATION_ERROR"),
        (PluginTransportError, "PLUGIN_TRANSPORT_ERROR"),
        (PluginDispatchError, "PLUGIN_DISPATCH_ERROR"),
        (PluginCapabilityError, "PLUGIN_CAPABILITY_ERROR"),
    ],
)
def test_subclass_default_codes(cls: type[PluginError], expected_code: str) -> None:
    err = cls()
    assert err.code == expected_code
    assert isinstance(err, PluginError)


def test_subclasses_accept_custom_message() -> None:
    err = PluginCapabilityError(message="capability X broke")
    assert err.code == "PLUGIN_CAPABILITY_ERROR"
    assert err.message == "capability X broke"


def test_subclass_code_override_wins() -> None:
    err = PluginRegistrationError("CUSTOM", "oops")
    assert err.code == "CUSTOM"
    assert err.message == "oops"
