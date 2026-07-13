# everything-control

Typed synchronous Python client for the versioned `everythingd` HTTP API. Rust remains the execution and state authority; this package is only a control-plane client.

## Development

```bash
python -m pip install -e '.[dev]'
everything-control --help
pytest
```

Start `everythingd` for the workspace before invoking live commands. The default endpoint is `http://127.0.0.1:3472`; override it with `--base-url` or the `base_url` constructor argument.

```python
from everything_control import EverythingClient

client = EverythingClient(".")
print(client.doctor())
response = client.plan("Map the runtime state transitions", mode="balanced")
print(response.run_id)
```


## Typed tool execution

```python
from everything_control import (
    EverythingClient,
    PatchExecutionRequest,
    VerificationCommand,
)

client = EverythingClient(".")
print([tool.tool_id for tool in client.tools()])

result = client.execute_patch(
    PatchExecutionRequest(
        objective="Update the greeting",
        relative_path="demo.txt",
        expected_content_hash="<blake3>",
        replacement_content="hello
",
        verification_commands=[
            VerificationCommand(
                program="cargo",
                args=["test", "--workspace"],
                label="workspace tests",
            )
        ],
        approval_granted=True,
    )
)
print(result.run_id, result.status, result.rolled_back)
```

Workspace mutation and process execution require explicit approval. Protected runtime/Git metadata cannot be accessed through workspace tools.
