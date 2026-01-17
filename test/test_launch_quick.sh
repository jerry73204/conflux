#!/bin/bash
# Quick test of conflux launch files
# Tests that each launch file starts correctly and subscribes to expected topics

set -e

# Test a single launch file
test_launch() {
    local launch_file="$1"
    local node_name="$2"

    echo "--- Testing $launch_file ---"

    # Start in background, redirect output
    timeout 10 play_launch launch conflux "$launch_file" > /tmp/conflux_test.log 2>&1 &
    local pid=$!

    # Wait for node to start
    sleep 2

    # Check node exists
    if ros2 node list 2>/dev/null | grep -q "$node_name"; then
        echo "✓ Node /$node_name running"

        # Show subscriptions
        ros2 node info "/$node_name" 2>/dev/null | grep -A 20 "Subscribers:" | grep -B 20 "Publishers:" | head -n -1
        echo ""
    else
        echo "✗ Node /$node_name not found"
    fi

    # Cleanup
    kill $pid 2>/dev/null || true
    pkill -f "conflux" 2>/dev/null || true
    sleep 1
}

echo "=== Quick Launch File Tests ==="
echo ""

test_launch "conflux.launch.xml" "conflux"
test_launch "realtime_sync.launch.xml" "conflux_realtime"
test_launch "offline_sync.launch.xml" "conflux_offline"
test_launch "multi_sensor_fusion.launch.xml" "conflux_fusion"
test_launch "stereo_camera.launch.xml" "conflux_stereo"
test_launch "low_frequency_localization.launch.xml" "conflux_localization"

echo "=== All tests completed ==="
