#!/bin/bash
# Test all conflux launch files
#
# Tests that each launch file starts correctly and subscribes to expected topics

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFLUX_DIR="$(dirname "$SCRIPT_DIR")"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

PASSED=0
FAILED=0

test_launch() {
    local launch_file="$1"
    local node_name="$2"
    shift 2
    local expected_topics=("$@")

    echo ""
    echo "=== Testing $launch_file ==="

    # Start in background
    timeout 8 play_launch launch conflux "$launch_file" > /dev/null 2>&1 &
    local pid=$!
    sleep 3

    # Check node exists
    if ros2 node list 2>/dev/null | grep -q "/$node_name"; then
        echo -e "${GREEN}✓ Node /$node_name running${NC}"

        # Check subscriptions
        local node_info=$(ros2 node info "/$node_name" 2>/dev/null)
        local all_found=true

        for topic in "${expected_topics[@]}"; do
            if echo "$node_info" | grep -q "$topic"; then
                echo -e "${GREEN}  ✓ Subscribed to $topic${NC}"
            else
                echo -e "${RED}  ✗ Missing subscription: $topic${NC}"
                all_found=false
            fi
        done

        if $all_found; then
            ((PASSED++))
        else
            ((FAILED++))
        fi
    else
        echo -e "${RED}✗ Node /$node_name not found${NC}"
        # Check error log
        if [ -f "$CONFLUX_DIR/play_log/latest/node/$node_name/err" ]; then
            echo "Error log:"
            cat "$CONFLUX_DIR/play_log/latest/node/$node_name/err"
        fi
        ((FAILED++))
    fi

    # Cleanup
    kill $pid 2>/dev/null || true
    pkill -9 -f "conflux" 2>/dev/null || true
    sleep 1
}

echo "=========================================="
echo "  conflux Launch File Tests"
echo "=========================================="

test_launch "conflux.launch.xml" "conflux" \
    "/camera/image_raw" "/lidar/points"

test_launch "realtime_sync.launch.xml" "conflux_realtime" \
    "/camera/image_raw" "/lidar/points"

test_launch "offline_sync.launch.xml" "conflux_offline" \
    "/camera/image_raw" "/lidar/points"

test_launch "multi_sensor_fusion.launch.xml" "conflux_fusion" \
    "/camera/image_raw" "/lidar/points" "/imu/data"

test_launch "stereo_camera.launch.xml" "conflux_stereo" \
    "/stereo/left/image_raw" "/stereo/right/image_raw"

test_launch "low_frequency_localization.launch.xml" "conflux_localization" \
    "/gps/fix" "/odom" "/imu/mag"

echo ""
echo "=========================================="
echo "  Summary"
echo "=========================================="
echo -e "${GREEN}Passed: $PASSED${NC}"
echo -e "${RED}Failed: $FAILED${NC}"

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}Some tests failed${NC}"
    exit 1
fi
