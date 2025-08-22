build target:
  @echo building {{target}}
  cargo zigbuild --target aarch64-unknown-linux-gnu --release -p {{target}}

deb target: (build target)
  @echo debbing {{target}}
  cargo deb --no-build --no-strip -p {{target}} --target aarch64-unknown-linux-gnu -o ./target/deb/{{target}}.deb

deploy target: (deb target)
  #!/usr/bin/env bash
  if [ -z "$ORB_IP" ]; then
    echo "Error: ORB_IP must be provided" >&2
    exit 1
  fi

  service_file=$(ls -1 ./{{target}}/debian/*.service 2>/dev/null | head -n1)
  if [ -n "$service_file" ]; then
      filename=$(basename "$service_file" .service)
      service_name="${filename##*.}"
  else
      echo "Error: No service file found in {{target}}/debian/" >&2
      exit 1
  fi

  echo "deploying $service_name to orb with ip $ORB_IP"

  if ! ssh -S ~/.ssh/orb-socket -O check worldcoin@$ORB_IP 2>/dev/null; then
    ssh -M -S ~/.ssh/orb-socket -fN worldcoin@$ORB_IP
  fi

  scp -o ControlPath=~/.ssh/orb-socket ./target/deb/{{target}}.deb worldcoin@$ORB_IP:/home/worldcoin
  ssh -o ControlPath=~/.ssh/orb-socket worldcoin@$ORB_IP sudo systemctl stop $service_name
  ssh -o ControlPath=~/.ssh/orb-socket worldcoin@$ORB_IP sudo apt install --reinstall ./{{target}}.deb -y
  ssh -o ControlPath=~/.ssh/orb-socket worldcoin@$ORB_IP sudo systemctl daemon-reload
  ssh -o ControlPath=~/.ssh/orb-socket worldcoin@$ORB_IP sudo systemctl start $service_name

  echo \n finished deploying {{target}}, service: $service_name

lint:
  cargo clippy --all --all-features --all-targets --no-deps -- -D warnings
  cargo fmt
