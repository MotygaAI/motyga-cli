"""Python SDK for running Motyga workflows.

Start with :class:`Motyga` for synchronous applications or
:class:`AsyncMotyga` for async applications. Most programs create a thread and
run a turn::

    from motyga_sdk import Motyga, Sandbox

    with Motyga() as motyga:
        thread = motyga.thread_start(sandbox=Sandbox.workspace_write)
        result = thread.run("Describe this project.")
        print(result.final_response)
"""

from ._version import __version__
from .api import (
    ApprovalMode,
    AsyncChatgptLoginHandle,
    AsyncMotyga,
    AsyncDeviceCodeLoginHandle,
    AsyncThread,
    AsyncTurnHandle,
    ChatgptLoginHandle,
    Motyga,
    DeviceCodeLoginHandle,
    ImageInput,
    Input,
    InputItem,
    LocalImageInput,
    MentionInput,
    RunInput,
    Sandbox,
    SkillInput,
    TextInput,
    Thread,
    TurnHandle,
    TurnResult,
)
from .client import MotygaConfig
from .errors import (
    MotygaError,
    MotygaRpcError,
    InternalRpcError,
    InvalidParamsError,
    InvalidRequestError,
    JsonRpcError,
    MethodNotFoundError,
    ParseError,
    RetryLimitExceededError,
    ServerBusyError,
    TransportClosedError,
    is_retryable_error,
)
from .retry import retry_on_overload

__all__ = [
    "__version__",
    "MotygaConfig",
    "Motyga",
    "AsyncMotyga",
    "ApprovalMode",
    "Sandbox",
    "ChatgptLoginHandle",
    "DeviceCodeLoginHandle",
    "AsyncChatgptLoginHandle",
    "AsyncDeviceCodeLoginHandle",
    "Thread",
    "AsyncThread",
    "TurnHandle",
    "AsyncTurnHandle",
    "TurnResult",
    "Input",
    "InputItem",
    "RunInput",
    "TextInput",
    "ImageInput",
    "LocalImageInput",
    "SkillInput",
    "MentionInput",
    "retry_on_overload",
    "MotygaError",
    "TransportClosedError",
    "JsonRpcError",
    "MotygaRpcError",
    "ParseError",
    "InvalidRequestError",
    "MethodNotFoundError",
    "InvalidParamsError",
    "InternalRpcError",
    "ServerBusyError",
    "RetryLimitExceededError",
    "is_retryable_error",
]
