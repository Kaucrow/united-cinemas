let socket = null;
const output = document.getElementById('output');
const connStatus = document.getElementById('status');
const broadcastBtn = document.getElementById('broadcastBtn');
const joinSessionBtn = document.getElementById('joinSessionBtn');
const streamNameInput = document.getElementById('streamName');
const videoFileInput = document.getElementById('videoFile');
const sourceCamera = document.getElementById('sourceCamera');
const sourceVideo = document.getElementById('sourceVideo');
const videoFileContainer = document.getElementById('videoFileContainer');
var pc = null;

const WS_URL = 'ws://localhost:8080/ws'

// Show/hide video file input based on selection
sourceCamera.addEventListener('change', function() {
  videoFileContainer.style.display = 'none';
});

sourceVideo.addEventListener('change', function() {
  videoFileContainer.style.display = 'block';
});

async function startSession(sessionType) {
  console.log('Starting session...');
  const streamName = streamNameInput.value.trim();

  if (!streamName) {
    addToOutput('Please enter a stream name.');
    return;
  }
  await connectWebSocket();
  console.log('WebSocket connected!');
  sendOffer(sessionType, streamName);
}

async function connectWebSocket() {
  return new Promise((resolve, reject) => {
    try {
      updateStatus('connecting', 'Connecting...');
      broadcastBtn.disabled = true;
      joinSessionBtn.disabled = true;
      streamNameInput.disabled = true;
 
      socket = new WebSocket(WS_URL);

      socket.onopen = function(event) {
        updateStatus('connected', 'Connected to WebSocket');
        addToOutput('Connected to WebSocket server');
        resolve();
      };

      socket.onmessage = function(event) {
        addToOutput('Received: ' + event.data);
        setRemoteDescription(event.data);
      };

      socket.onclose = function(event) {
        updateStatus('disconnected', 'Disconnected');
        broadcastBtn.disabled = false;
        joinSessionBtn.disabled = false;
        addToOutput('Disconnected from WebSocket');
      };
 
      socket.onerror = function(error) {
        updateStatus('disconnected', 'Connection Error');
        addToOutput('WebSocket error: ' + error);
        broadcastBtn.disabled = false;
        joinSessionBtn.disabled = false;
        reject();
      };
    } catch (error) {
      addToOutput('Failed to connect: ' + error);
      updateStatus('disconnected', 'Connection Failed');
      broadcastBtn.disabled = false;
      joinSessionBtn.disabled = false;
      reject();
    }
  });
}

function sendOffer(sessionType, streamName) {
  pc = new RTCPeerConnection({
    iceServers: [
      {
        urls: 'stun:stun.l.google.com:19302'
      }
    ]
  });

  pc.oniceconnectionstatechange = e => addToOutput(pc.iceConnectionState);

  pc.onicecandidate = event => {
  if (event.candidate === null) {
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      console.warn('socket not open, cannot send offer');
      return;
    }

    const innerSdpB64 = btoa(JSON.stringify(pc.localDescription));

    const payload = {
      action: sessionType,   
      name: streamName || 'default',
      sdp: innerSdpB64
    };

    const outer = btoa(JSON.stringify(payload));

    socket.send(outer);
    addToOutput(`Sent payload for ${payload.name} (${payload.action})`);
  }
};

    if (sessionType === 'broadcast') {
    const broadcastSource = document.querySelector('input[name="broadcastSource"]:checked').value;
    
    if (broadcastSource === 'video') {
      // Use video file
      const file = videoFileInput.files[0];
      if (!file) {
        addToOutput('Please select a video file first!');
        return;
      }
      
      addToOutput(`Loading video file: ${file.name}`);
      const videoElement = document.getElementById('video1');
      const fileURL = URL.createObjectURL(file);
      
      videoElement.src = fileURL;
      videoElement.loop = true; // Loop the video
      videoElement.muted = true; // Mute to avoid feedback
      
      videoElement.onloadedmetadata = function() {
        addToOutput('Video file loaded, starting stream...');
        
        // Wait for video to be ready to play
        videoElement.play()
          .then(() => {
            // Capture stream from video element
            const stream = videoElement.captureStream ? videoElement.captureStream() : videoElement.mozCaptureStream();
            
            if (!stream) {
              addToOutput('Error: Browser does not support capturing stream from video element');
              return;
            }
            
            // Add video tracks to peer connection
            stream.getVideoTracks().forEach(track => {
              pc.addTrack(track, stream);
              addToOutput('Added video track from file to peer connection');
            });
            
            // Create and set offer
            pc.createOffer()
              .then(d => pc.setLocalDescription(d))
              .catch(e => addToOutput('Error creating offer: ' + e));
          })
          .catch(e => addToOutput('Error playing video: ' + e));
      };
      
      videoElement.onerror = function() {
        addToOutput('Error loading video file');
        URL.revokeObjectURL(fileURL);
      };
      
    } else {
      // Use camera
      navigator.mediaDevices.getUserMedia({ video: true, audio: false })
        .then(stream => {
          stream.getTracks().forEach(track => pc.addTrack(track, stream));
          document.getElementById('video1').srcObject = stream;
          pc.createOffer()
            .then(d => pc.setLocalDescription(d))
            .catch(addToOutput)
        }).catch(addToOutput)
    }
  } else {
    // For viewers, add transceivers for both video and audio
    pc.addTransceiver('video');
    pc.addTransceiver('audio');  // Added audio transceiver

    pc.createOffer()
      .then(d => pc.setLocalDescription(d))
      .catch(addToOutput)

    pc.ontrack = function (event) {
      var el = document.getElementById('video1');
      
      // Check if we already have a stream attached
      if (!el.srcObject) {
        el.srcObject = new MediaStream();
      }
      
      // Add the incoming track to our media stream
      el.srcObject.addTrack(event.track);
      el.autoplay = true;
      el.controls = true;  // Changed to true so users can control audio
      
      addToOutput(`Received ${event.track.kind} track from broadcast`);
    }

    // Handle connection state changes for better debugging
    pc.onconnectionstatechange = function() {
      addToOutput('Connection state: ' + pc.connectionState);
    }
  }
}

function setRemoteDescription(remoteDescription) {
  if (!pc) return;

  try {
    // remoteDescription is expected to be a base64 string sent by server
    const decoded = atob(remoteDescription);
    const parsed = JSON.parse(decoded);

    if (parsed.type && parsed.sdp) {
      pc.setRemoteDescription(new RTCSessionDescription(parsed))
        .then(() => addToOutput('Remote description (direct) set'))
        .catch(e => addToOutput('Failed to set remote description: ' + e));
      return;
    }

    if (parsed.sdp) {
      // parsed.sdp is expected to be base64 of the inner RTCSessionDescription JSON
      const inner = JSON.parse(atob(parsed.sdp));
      pc.setRemoteDescription(new RTCSessionDescription(inner))
        .then(() => addToOutput('Remote description (from payload) set'))
        .catch(e => addToOutput('Failed to set remote description: ' + e));
      return;
    }

    throw new Error('Unknown remote description format');
  } catch (e) {
    console.error(e);
    alert('Failed to set remote description: ' + e);
  }
}

function sendMessage() {
  if (socket && socket.readyState === WebSocket.OPEN) {
    const message = 'Hello WebSocket! Current time: ' + new Date().toLocaleTimeString();
    socket.send(message);
    addToOutput('Sent: ' + message);
  } else {
    addToOutput('Cannot send message - WebSocket is not connected');
  }
}

function disconnectWebSocket() {
  if (socket) {
    socket.close();
    socket = null;
  }
}

function addToOutput(text) {
  const timestamp = new Date().toLocaleTimeString();
  output.innerHTML += `<div><strong>[${timestamp}]</strong> ${text}</div>`;
  output.scrollTop = output.scrollHeight;
}

function updateStatus(state, message) {
  connStatus.textContent = message;
  connStatus.className = 'status ' + state;
}