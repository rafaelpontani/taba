#!/bin/sh
# Inside the container: installs the AWS SDK for JavaScript v3 of the variant (SDK_DIR: sdk-current or sdk-old) with
# `npm ci`, which takes exactly the versions and the hashes of its package-lock.json, and runs the driver.
mkdir /app && cd /app && cp /repo/connectors/gateway-contract/drivers/node/driver.js .
cp "/repo/connectors/gateway-contract/drivers/node/$SDK_DIR/package.json" "/repo/connectors/gateway-contract/drivers/node/$SDK_DIR/package-lock.json" .
npm ci --silent --no-audit --no-fund >/tmp/npm.log 2>&1 || { tail -5 /tmp/npm.log; exit 3; }
exec node driver.js "$@"
