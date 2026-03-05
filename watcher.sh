#!/bin/sh
# Cave VM Watcher - auto-starts VMs, updates IP cache and SSH config
CAVE="/home/colindaniels/cave/target/release/cave"
VMS_DIR="/home/colindaniels/cave/vms"
LOG="/home/colindaniels/cave/watcher.log"

log() {
    echo "$(date '+%Y-%m-%d %H:%M:%S') [watcher] $1" >> "$LOG"
}

log "Watcher started, configs in $VMS_DIR"

while true; do
    # Poll network to update IP cache and SSH config
    "$CAVE" poll 2>/dev/null

    for conf in "$VMS_DIR"/*.conf; do
        [ -f "$conf" ] || continue
        hostname=$(basename "$conf" .conf)
        # Call cave watcher-start which uses the same code as deploy
        if "$CAVE" watcher-start "$hostname" 2>/dev/null; then
            log "Started VM on $hostname"
        fi
    done
    sleep 10
done
