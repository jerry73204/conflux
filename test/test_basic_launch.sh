#!/bin/bash
# Test basic msync launch with mock publishers
#
# This script:
# 1. Launches msync with the example config
# 2. Publishes mock Image and PointCloud2 messages
# 3. Checks for synchronized output

set -e

echo "=== Testing msync.launch.xml ==="
echo ""

# Create a temporary directory for logs
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# Start msync in background
echo "Starting msync node..."
play_launch launch msync msync.launch.xml log_level:=debug &
LAUNCH_PID=$!
sleep 3

# Check if node is running
if ! ros2 node list | grep -q msync; then
    echo "ERROR: msync node not found"
    kill $LAUNCH_PID 2>/dev/null || true
    exit 1
fi
echo "msync node is running"

# List topics
echo ""
echo "Available topics:"
ros2 topic list

# Check subscribed topics
echo ""
echo "Checking subscriptions..."
ros2 topic info /camera/image_raw 2>/dev/null || echo "  /camera/image_raw - waiting for publisher"
ros2 topic info /lidar/points 2>/dev/null || echo "  /lidar/points - waiting for publisher"

# Publish test messages
echo ""
echo "Publishing test messages..."

# Get current time for header stamps
SEC=$(date +%s)
NANOSEC=0

# Publish camera image (5 messages at 30Hz-ish)
for i in {1..5}; do
    ros2 topic pub --once /camera/image_raw sensor_msgs/msg/Image \
        "{header: {stamp: {sec: $SEC, nanosec: $((i * 33000000))}, frame_id: 'camera'}, height: 480, width: 640, encoding: 'bgr8', is_bigendian: 0, step: 1920, data: []}" \
        2>/dev/null &
done

# Publish lidar points (2 messages at 10Hz-ish)
for i in {1..2}; do
    ros2 topic pub --once /lidar/points sensor_msgs/msg/PointCloud2 \
        "{header: {stamp: {sec: $SEC, nanosec: $((i * 100000000))}, frame_id: 'lidar'}, height: 1, width: 100, is_bigendian: 0, point_step: 16, row_step: 1600, data: [], is_dense: 1}" \
        2>/dev/null &
done

sleep 2

# Check output topic
echo ""
echo "Checking output topic /synchronized..."
ros2 topic info /synchronized 2>/dev/null || echo "  Output topic not yet created (may need matching messages)"

# Show node info
echo ""
echo "Node info:"
ros2 node info /msync 2>/dev/null | head -20

# Cleanup
echo ""
echo "Stopping msync..."
kill $LAUNCH_PID 2>/dev/null || true
wait $LAUNCH_PID 2>/dev/null || true

echo ""
echo "=== Test completed ==="
