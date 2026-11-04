#!/bin/sh
# Inside the container: installs boto3 (PACKAGES) and runs the driver.
pip install -q --disable-pip-version-check $PACKAGES >/dev/null 2>&1 || { echo "pip install failed: $PACKAGES" >&2; exit 3; }
exec python /repo/connectors/gateway-contract/drivers/python/driver.py "$@"
