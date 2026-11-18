#!/bin/bash
# scale_export.sh <dir>: one export of <dir>/src with the pcv binary into <dir>/out
. ~/env.sh; . ~/.cargo/env
d=$HOME/runs/$1; export SYNTH_ROUTER_TIMEOUT_SECS=900 SYNTH_USER_REGISTRY_DIR=$d/user-registry
cd $d/src; rm -rf $d/out
$HOME/pt/pcv/target/release/synth export-kicad i2c_sensor_node.synth --out $d/out --router kicad-routing-tools --kicad-routing-tools-repo $KICAD_ROUTING_TOOLS_REPO --kicad-routing-tools-python $SYNTH_KRT_PYTHON > $d/export.log 2>&1
echo "exit=$?" >> $d/export.log
