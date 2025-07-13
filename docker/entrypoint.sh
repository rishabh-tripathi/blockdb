#!/bin/bash
set -e

# Default configuration
BLOCKDB_CONFIG=${BLOCKDB_CONFIG:-/etc/blockdb/blockdb.toml}
BLOCKDB_DATA_DIR=${BLOCKDB_DATA_DIR:-/var/lib/blockdb}
BLOCKDB_LOG_LEVEL=${BLOCKDB_LOG_LEVEL:-info}

# Function to update configuration from environment variables
update_config() {
    local config_file="$1"
    
    # Update node ID if provided
    if [ -n "$BLOCKDB_NODE_ID" ]; then
        sed -i "s/node_id = \".*\"/node_id = \"$BLOCKDB_NODE_ID\"/" "$config_file"
    fi
    
    # Update server host and port
    if [ -n "$BLOCKDB_HOST" ]; then
        sed -i "s/host = \".*\"/host = \"$BLOCKDB_HOST\"/" "$config_file"
    fi
    
    if [ -n "$BLOCKDB_PORT" ]; then
        sed -i "s/port = .*/port = $BLOCKDB_PORT/" "$config_file"
    fi
    
    # Update data directory
    if [ -n "$BLOCKDB_DATA_DIR" ]; then
        sed -i "s|data_dir = \".*\"|data_dir = \"$BLOCKDB_DATA_DIR\"|" "$config_file"
    fi
    
    # Update cluster peers if provided
    if [ -n "$BLOCKDB_PEERS" ]; then
        # Convert comma-separated list to TOML array
        peers_array=$(echo "$BLOCKDB_PEERS" | sed 's/,/", "/g' | sed 's/^/["/' | sed 's/$/"]/')
        sed -i "s/peers = \[\]/peers = $peers_array/" "$config_file"
    fi
    
    # Update heartbeat interval
    if [ -n "$BLOCKDB_HEARTBEAT_INTERVAL" ]; then
        sed -i "s/heartbeat_interval = .*/heartbeat_interval = $BLOCKDB_HEARTBEAT_INTERVAL/" "$config_file"
    fi
    
    # Update election timeout
    if [ -n "$BLOCKDB_ELECTION_TIMEOUT" ]; then
        sed -i "s/election_timeout = .*/election_timeout = $BLOCKDB_ELECTION_TIMEOUT/" "$config_file"
    fi
    
    # Update memory settings
    if [ -n "$BLOCKDB_MEMTABLE_SIZE" ]; then
        sed -i "s/memtable_size_limit = .*/memtable_size_limit = $BLOCKDB_MEMTABLE_SIZE/" "$config_file"
    fi
    
    # Update log level
    if [ -n "$BLOCKDB_LOG_LEVEL" ]; then
        sed -i "s/level = \".*\"/level = \"$BLOCKDB_LOG_LEVEL\"/" "$config_file"
    fi
}

# Function to wait for other nodes
wait_for_peers() {
    if [ -n "$BLOCKDB_PEERS" ] && [ "$BLOCKDB_WAIT_FOR_PEERS" = "true" ]; then
        echo "Waiting for peer nodes to be ready..."
        IFS=',' read -ra PEER_ARRAY <<< "$BLOCKDB_PEERS"
        for peer in "${PEER_ARRAY[@]}"; do
            # Extract host and port
            host=$(echo "$peer" | cut -d':' -f1)
            port=$(echo "$peer" | cut -d':' -f2)
            
            echo "Waiting for $host:$port..."
            until curl -f "http://$host:$port/health" >/dev/null 2>&1; do
                echo "Peer $host:$port not ready, waiting..."
                sleep 5
            done
            echo "Peer $host:$port is ready"
        done
        echo "All peers are ready"
    fi
}

# Function to initialize data directory
init_data_dir() {
    echo "Initializing data directory: $BLOCKDB_DATA_DIR"
    mkdir -p "$BLOCKDB_DATA_DIR"
    chown -R blockdb:blockdb "$BLOCKDB_DATA_DIR"
    
    # Create log directory
    mkdir -p /var/log/blockdb
    chown -R blockdb:blockdb /var/log/blockdb
}

# Main execution
case "$1" in
    server)
        echo "Starting BlockDB Server..."
        
        # Initialize data directory
        init_data_dir
        
        # Create a working copy of the config
        cp "$BLOCKDB_CONFIG" /tmp/blockdb.toml
        
        # Update configuration from environment
        update_config /tmp/blockdb.toml
        
        # Wait for peers if configured
        wait_for_peers
        
        # Start the server
        exec blockdb-server --config /tmp/blockdb.toml
        ;;
    cli)
        shift
        exec blockdb-cli "$@"
        ;;
    bash)
        exec /bin/bash
        ;;
    *)
        echo "Usage: $0 {server|cli|bash}"
        echo "  server: Start BlockDB server"
        echo "  cli: Run BlockDB CLI commands"
        echo "  bash: Start interactive shell"
        exit 1
        ;;
esac