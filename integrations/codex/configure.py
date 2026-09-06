"""Print a hooks.json snippet; never overwrite client config or trust hooks."""
import argparse
import json
from pathlib import Path
import shlex

parser = argparse.ArgumentParser()
parser.add_argument("--binary", required=True)
parser.add_argument("--database", required=True)
parser.add_argument("--session", required=True)
parser.add_argument("--external-session", required=True)
args = parser.parse_args()
if not Path(args.binary).is_absolute() or not Path(args.database).is_absolute():
    parser.error("binary and database must be absolute paths")
command = shlex.join([args.binary, "--database", args.database, "--session", args.session,
                      "--source", "codex", "--client", "codex",
                      "--external-session", args.external_session])
events = ["SessionStart", "SessionEnd", "UserPromptSubmit", "PreToolUse",
          "PostToolUse", "Stop", "Interrupt"]
print(json.dumps({"hooks": {event: [{"hooks": [{"type": "command", "command": command,
                      "timeout": 3}]}] for event in events}}, indent=2))
