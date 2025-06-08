#!/bin/bash

echo "Testing Enhanced Progress Display with CLI Pull Operation"
echo "========================================================="

# Test with a real registry pull to see the enhanced progress display
echo "Running enhanced progress test with a sample image..."
echo ""

# Use the debug build for faster iteration
cd /home/zhou/codes/docker-image-pusher

echo "Command: ./target/debug/docker-image-pusher pull -i registry.cn-beijing.aliyuncs.com/yoce/python:3.11-slim --max-concurrent 6"
echo ""

# Run the actual test with a multi-layer image
./target/debug/docker-image-pusher pull -i registry.cn-beijing.aliyuncs.com/yoce/python:3.11-slim --max-concurrent 6

echo ""
echo "Test completed! Check the output above for enhanced progress display."
echo ""
echo "Expected features:"
echo "- ✅ Real-time progress bars: 🚀 [🟩🟩🟩░░░░░░░] XX%"
echo "- ✅ Task counters: T:X/Y A:Z"
echo "- ✅ Concurrency indicators: ⚡X/Y"
echo "- ✅ Speed measurements: 📈X.XMB/s"
echo "- ✅ Strategy display: S:SF"
echo "- ✅ Auto-adjustment: 🔧AUTO"
echo "- ✅ ETA predictions: ETA:Xm Ys(XX%)"
