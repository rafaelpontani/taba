#!/usr/bin/env bash
# Builds the Taba driver for the OpenMessaging Benchmark on Java 11, in a container (no Java needed on the host):
# checks out the OMB commit of OMB_VERSION, installs its driver-api and the Java connector of this commit, runs the
# driver's unit tests and packages connectors/omb/target/driver-taba-all.jar (driver and connector in one JAR).
#   connectors/omb/build.sh
# The Maven repository is kept in the Docker volume taba-omb-m2; the OMB checkout in connectors/omb/.work/ (not versioned).
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

omb_checkout
omb_maven "mvn -q -B -f /repo/connectors/omb/.work/omb/pom.xml -pl driver-api -am install -DskipTests -Dspotbugs.skip -Dcheckstyle.skip -Dlicense.skip -Dspotless.check.skip=true -Dmaven.javadoc.skip=true && \
  mvn -q -B -f /repo/connectors/java/pom.xml install -DskipTests && \
  mvn -B -f /repo/connectors/omb/pom.xml package"
echo "built: connectors/omb/target/driver-taba-all.jar"
