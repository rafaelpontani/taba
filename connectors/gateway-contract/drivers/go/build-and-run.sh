#!/bin/sh
# Inside the container: fetches the SDK modules (GET), builds the driver (TAGS picks the old-SDK files) and runs it.
mkdir /app && cd /app && cp /repo/connectors/gateway-contract/drivers/go/*.go .
go mod init gwdriver >/dev/null 2>&1
go get $GET >/tmp/get.log 2>&1 || { cat /tmp/get.log; exit 3; }
go build $TAGS -o driver . || exit 3
exec ./driver "$@"
