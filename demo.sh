#!/bin/bash
# 批量发送 JSON 命令到 veritasd
commands=(
  '{"cmd":"create_object"}'
  '{"cmd":"tx_begin","actor_id":1}'
  '{"cmd":"tx_create_object","session_id":1}'
  '{"cmd":"tx_write","session_id":1,"state_id":0,"value":"hello world","object_id":2}'
  '{"cmd":"tx_read","session_id":1,"state_id":0}'
  '{"cmd":"tx_commit","session_id":1}'
  '{"cmd":"list_objects"}'
  '{"cmd":"world_info"}'
)

for cmd in "${commands[@]}"; do
  echo "$cmd"
  sleep 0.3
done | ./target/release/veritasd
