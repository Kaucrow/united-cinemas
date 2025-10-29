let socket = null;
const output = document.getElementById('output');
const connStatus = document.getElementById('status');
const broadcastBtn = document.getElementById('broadcastBtn');
const joinSessionBtn = document.getElementById('joinSessionBtn');
var pc = null;

const WS_URL = 'ws://localhost:8080/ws'

async function startSession(isPublisher) {
  console.log('Starting session...');
  await connectWebSocket();
  console.log('WebSocket connected!');
  sendOffer(isPublisher);
}

async function connectWebSocket() {
  return new Promise((resolve, reject) => {
    try {
      updateStatus('connecting', 'Connecting...');
      broadcastBtn.disabled = true;
      joinSessionBtn.disabled = true;
 
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

function sendOffer(isPublisher) {
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
      if (socket && socket.readyState === WebSocket.OPEN) {
        const offer = btoa(JSON.stringify(pc.localDescription));
        socket.send(offer);
        addToOutput('Sent: ' + offer);
      } else {
        addToOutput('Cannot send message - WebSocket is not connected.');
      }
    }
  }

  if (isPublisher) {
    navigator.mediaDevices.getUserMedia({ video: true, audio: false })
      .then(stream => {
        stream.getTracks().forEach(track => pc.addTrack(track, stream));
        document.getElementById('video1').srcObject = stream;
        pc.createOffer()
          .then(d => pc.setLocalDescription(d))
          .catch(addToOutput)
      }).catch(addToOutput)
  } else {
    pc.addTransceiver('video');
    pc.createOffer()
      .then(d => pc.setLocalDescription(d))
      .catch(addToOutput)

    pc.ontrack = function (event) {
      var el = document.getElementById('video1');
      el.srcObject = event.streams[0];
      el.autoplay = true;
      el.controls = false;
    }
  }
}

function setRemoteDescription(remoteDescription) {
  if (!pc) return;

  try {
    pc.setRemoteDescription(new RTCSessionDescription(JSON.parse(atob(remoteDescription))));
  } catch (e) {
    alert(e);
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