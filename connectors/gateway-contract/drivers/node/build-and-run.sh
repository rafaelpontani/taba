#!/bin/sh
# Inside the container: installs the AWS SDK for JavaScript v3 (SDK_VERSION) and runs the driver.
mkdir /app && cd /app && cp /repo/connectors/gateway-contract/drivers/node/driver.js .
npm i --no-save --silent --no-audit --no-fund "@aws-sdk/client-sqs@$SDK_VERSION" "@aws-sdk/client-sns@$SDK_VERSION" >/tmp/npm.log 2>&1 || { tail -5 /tmp/npm.log; exit 3; }
exec node driver.js "$@"
