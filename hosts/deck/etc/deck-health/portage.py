#!/usr/bin/env python3
"""deck-health portage: is the system keeping up with Gentoo? Writes /var/lib/deck-health/portage.json.

Run by etc/systemd/system/deck-portage.service: after the daily sync (gentoo-sync.service) and
after every emerge that changes something (the emerge wrapper, usr-local/sbin/emerge). Root reads
what the panel can't (emerge.log and the preserved-libs registry are root/portage only); the
panel's checker (~/.local/bin/deck-attention) turns it into tile lines:

  world_updated     when `emerge --update ... @world` last finished successfully (null: never)
  preserved         libraries kept for packages still linked to them: `emerge @preserved-rebuild`
  depclean_blocked  `emerge --pretend --depclean` refuses to run, usually because world has
                    unsatisfied dependencies, which an @world update fixes
"""
import datetime
import json
import pathlib
import re
import subprocess
import sys

STATE = pathlib.Path("/var/lib/deck-health")
LOG = pathlib.Path("/var/log/emerge.log")
REGISTRY = pathlib.Path("/var/lib/portage/preserved_libs_registry")


def is_world_update(args):
    words = args.split()
    update = any(w == "--update" or re.fullmatch(r"-[a-zA-Z]*u[a-zA-Z]*", w) for w in words)
    pretend = any(w == "--pretend" or re.fullmatch(r"-[a-zA-Z]*p[a-zA-Z]*", w) for w in words)
    return update and not pretend and any(w in ("@world", "world") for w in words)


def last_world_update():
    """`1760000000:  *** emerge --update --deep @world` ... `1760001000:  *** exiting successfully.`"""
    started, last = None, None
    with LOG.open(errors="replace") as log:
        for line in log:
            stamp, _, text = line.partition(":")
            text = text.strip()
            if text.startswith("*** emerge "):
                started = is_world_update(text[len("*** emerge "):])
            elif text.startswith("*** exiting successfully") and started:
                last = int(stamp)
                started = None
            elif text.startswith(("*** exiting unsuccessfully", "*** terminating")):
                started = None
    return last


def main():
    status = {"checked": datetime.datetime.now().astimezone().isoformat(timespec="seconds")}

    try:
        t = last_world_update()
    except (OSError, ValueError):
        t = None
    status["world_updated"] = (datetime.datetime.fromtimestamp(t).astimezone().isoformat(timespec="seconds")
                               if t else None)

    try:
        status["preserved"] = len(json.loads(REGISTRY.read_text() or "{}"))
    except (OSError, ValueError):
        status["preserved"] = 0

    r = subprocess.run(["/usr/bin/emerge", "--pretend", "--depclean", "--quiet"],
                       capture_output=True, text=True)
    out = r.stdout + r.stderr
    status["depclean_blocked"] = r.returncode != 0
    status["depclean_reason"] = ""
    if r.returncode != 0:
        unsatisfied = list(dict.fromkeys(re.findall(r"^\s*\*?\s+(\S+) pulled in by:", out, re.M)))
        status["depclean_reason"] = (f"unsatisfied: {', '.join(unsatisfied[:3])}" if unsatisfied
                                     else out.strip().splitlines()[-1][:160] if out.strip() else "")

    tmp = STATE / "portage.json.new"
    tmp.write_text(json.dumps(status, indent=1) + "\n")
    tmp.rename(STATE / "portage.json")
    print(json.dumps(status, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
