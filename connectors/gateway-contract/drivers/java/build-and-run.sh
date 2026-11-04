#!/bin/sh
# Inside the container: resolves the SDK with Maven (FLAVOR v1 or v2, SDK_VERSION), compiles the driver with javac and runs it.
mkdir /app && cd /app && cp -r /repo/connectors/gateway-contract/drivers/java/src "/repo/connectors/gateway-contract/drivers/java/pom-$FLAVOR.xml" .
mvn -q -B -f "pom-$FLAVOR.xml" -Dsdk.version="$SDK_VERSION" dependency:build-classpath -Dmdep.outputFile=cp.txt >/tmp/mvn.log 2>&1 || { tail -15 /tmp/mvn.log; exit 3; }
mkdir out
OPS="OpsV2"
[ "$FLAVOR" = v1 ] && OPS="OpsV1"
javac -nowarn -cp "$(cat cp.txt)" -d out src/Main.java src/Json.java src/Ops.java "src/$OPS.java" 2>&1 | grep -v '^Note' | head -30
java -cp "out:$(cat cp.txt)" Main --flavor "$FLAVOR" --sdk "$SDK_VERSION" "$@" 2>&1 | grep -v 'SLF4J\|JAXB\|com.amazonaws.util.Base64 warn'
