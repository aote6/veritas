#!/bin/bash
i=0
while true; do
  echo "=== 周期 $i ==="
  {
    echo '{"cmd":"create_object"}'
    echo '{"cmd":"tx_begin","actor_id":1}'
    echo "{\"cmd\":\"tx_create_object\",\"session_id\":$((i+1))}"
    echo "{\"cmd\":\"tx_write\",\"session_id\":$((i+1)),\"state_id\":0,\"value\":\"cycle_$i\",\"object_id\":$((i+2))}"
    echo "{\"cmd\":\"tx_commit\",\"session_id\":$((i+1))}"
    echo '{"cmd":"list_objects"}'
  } | ./target/release/veritasd 2>/dev/null
  i=$((i+1))
  sleep 1
done
