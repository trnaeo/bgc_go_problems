#!/bin/bash
set -e
cd "$(dirname "$0")"

cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.31

ssh bgcuser@raspberrypi2 'mkdir -p /home/bgcuser/go_problems'

rsync -avz target/aarch64-unknown-linux-gnu/release/go_problems \
      bgcuser@raspberrypi2:/home/bgcuser/go_problems/go_problems

if [ -f secrets.env ]; then
  rsync -avz --chmod=F600 secrets.env \
        bgcuser@raspberrypi2:/home/bgcuser/go_problems/secrets.env
fi

if [ -f config.toml ]; then
  rsync -avz config.toml \
        bgcuser@raspberrypi2:/home/bgcuser/go_problems/config.toml
fi

echo "Deployed: $(date)"
