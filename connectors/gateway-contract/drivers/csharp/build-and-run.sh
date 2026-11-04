#!/bin/sh
# Inside the container: builds the driver against AWSSDK.SQS/SNS at SQS_VERSION/SNS_VERSION and runs it.
mkdir /app && cd /app && cp /repo/connectors/gateway-contract/drivers/csharp/*.cs /repo/connectors/gateway-contract/drivers/csharp/*.csproj .
dotnet build -c Release -o out -p:SqsVersion="$SQS_VERSION" -p:SnsVersion="$SNS_VERSION" -v q -nologo >/tmp/build.log 2>&1 || { grep -E ' error |Build FAILED' /tmp/build.log | head -20; exit 3; }
exec dotnet out/gwdriver.dll "$@"
