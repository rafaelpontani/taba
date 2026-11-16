#!/bin/sh
# Inside the container: installs boto3 of the variant (REQUIREMENTS: requirements-current.txt or requirements-old.txt)
# with --require-hashes, so pip takes exactly the versions and the files listed there, and runs the driver.
pip install -q --disable-pip-version-check --require-hashes -r "/repo/connectors/gateway-contract/drivers/python/$REQUIREMENTS" >/dev/null 2>&1 || { echo "pip install failed: $REQUIREMENTS" >&2; exit 3; }
exec python /repo/connectors/gateway-contract/drivers/python/driver.py "$@"
