const CLIENT_ID = 'v1.mqtt3/service-agents/js-client.example.org';
const PUBLISH_TOPIC = 'agents/alpha.janus-gateway.example.org/api/v1/in/conference.example.org';
const SUBSCRIBE_TOPIC = 'apps/janus-gateway.example.org/api/v1/responses';
const PLUGIN = 'janus.plugin.conference';
const CONNECT_TIMEOUT = 3;
const RESPONSE_TIMEOUT = 5;
const MEDIA_CONSTRAINTS = { audio: true, video: { width: { ideal: 1280 }, height: { ideal: 720 } } };

class JanusClient {
  // This must be called and awaited before using other methods.
  // It connects to the broker, subscribes to `SUBSCRIBE_TOPIC`, obtains the session id
  // and the plugin handle id which are necessary to make further requests.
  async init() {
    await this._connect();
    this.client.onMessageArrived = (message) => this._onMessageArrived(message);
    this.client.subscribe(SUBSCRIBE_TOPIC);
    this.sessionId = await this._initSession();
    this.handleId = await this._initHandle();
  }

  async _connect() {
    return new Promise((resolve, reject) => {
      this.client = new Paho.MQTT.Client('192.168.99.100', Number(8080), '/mqtt', CLIENT_ID);
      this.client.onConnectionLost = () => console.error("Connection to the MQTT broker lost");

      return this.client.connect({
        timeout: CONNECT_TIMEOUT,
        cleanSession: true,
        onSuccess: () => resolve(),
        onFailure: (err) => reject(err)
      });
    });
  }

  async _initSession() {
    let response = await this.request({ janus: 'create' });
    if (response.janus !== 'success') throw 'Failed to create session';
    return response.data.id;
  }

  async _initHandle() {
    let payload = { janus: 'attach', session_id: this.sessionId, plugin: PLUGIN };
    let response = await this.request(payload);
    if (response.janus !== 'success') throw 'Failed to create plugin handle';
    return response.data.id;
  }

  _onMessageArrived(message) {
    document.dispatchEvent(new CustomEvent('janusMessage', { detail: message }));
  }

  // Publishes a message with the given payload to `PUBLISH_TOPIC`.
  publish(payload) {
    let message = new Paho.MQTT.Message(JSON.stringify({ payload: JSON.stringify(payload) }));
    message.destinationName = PUBLISH_TOPIC;
    this.client.send(message);
  }

  // Adds random transaction id to `payload`, publishes it and returns the promise which resolves
  // with the response on this transaction.
  async request(payload) {
    payload.transaction = Math.floor(Math.random() * 10000000).toString();
    let responsePromise = this.getResponse(payload.transaction);
    console.debug(`Request ${payload.transaction}`, payload);
    this.publish(payload);
    return responsePromise;
  }

  // Returns a promise that resolves when a message on the given `transaction` arrives.
  // If no such message is received in `RESPONSE_TIMEOUT` seconds then the promise rejects.
  async getResponse(transaction) {
    return new Promise((resolve, reject) => {
      let eventHandler = (event) => {
        let payload = JSON.parse(JSON.parse(event.detail.payloadString).payload);

        if (payload.transaction === transaction) {
          clearTimeout(timeoutId);
          document.removeEventListener('janusMessage', eventHandler);
          console.debug(`Response ${transaction}`, payload);
          resolve(payload);
        }
      };

      let timeoutId = setTimeout(() => {
        document.removeEventListener('janusMessage', eventHandler, false);
        reject(`Response awaiting timed out for transaction ${transaction}`);
      }, RESPONSE_TIMEOUT * 1000);

      document.addEventListener('janusMessage', eventHandler, false);
    });
  }

  // Convenience wrapper around `request` to send a message request to the plugin handle
  // with optional JSEP. Expects ack response at first and then the event response.
  // Returns the event response.
  async requestMessage(body, jsep) {
    var payload = {
      janus: 'message',
      session_id: this.sessionId,
      handle_id: this.handleId,
      body: body
    };

    if (typeof (jsep) !== 'undefined') payload.jsep = jsep;

    let ackResponse = await this.request(payload);
    if (ackResponse.janus !== 'ack') throw 'Expected ack response';

    let response = await this.getResponse(payload.transaction);
    if (response.janus !== 'event') throw 'Expected event';
    return response;
  }

  // Sends local candidate to Janus and ensures ack response.
  async trickleIceCandidate(candidate) {
    let payload = {
      janus: 'trickle',
      session_id: this.sessionId,
      handle_id: this.handleId,
      candidate: candidate
    };

    return this.request(payload).then(payload => {
      if (payload.janus !== 'ack') throw 'Expected ack response';
    });
  }
}

// Start button click handler.
async function start() {
  // Init Janus client. Create session & plugin handle.
  let client = new JanusClient();
  await client.init();

  // Init PeerConnection.
  let peerConnection = new RTCPeerConnection(null);

  peerConnection.onicecandidate = (event) => {
    if (event.candidate) client.trickleIceCandidate(event.candidate);
  };

  // Get webcam stream, add it to the PeerConnection and set as video element src.
  let stream = await navigator.mediaDevices.getUserMedia(MEDIA_CONSTRAINTS).catch(err => {
    console.error("Failed to get local video. Is webcam enabled?", err);
    return;
  });

  peerConnection.addStream(stream);
  document.getElementById("localVideo").srcObject = stream;

  // Create SDP offer and set it as local description.
  let sdpOffer = await peerConnection.createOffer({ offerToReceiveVideo: false });
  sdpOffer.sdp = sdpOffer.sdp.replace("a=rtpmap:96 VP8/90000", "a=rtpmap:96 H264/90000");
  sdpOffer.sdp = sdpOffer.sdp.replace("a=rtpmap:98 VP9/90000", "a=rtpmap:98 H264/90000");
  peerConnection.setLocalDescription(sdpOffer);

  // Make `stream.create` request.
  let payload = { method: 'stream.create', id: 'test' }
  let jsep = { type: 'offer', sdp: sdpOffer.sdp };
  let response = await client.requestMessage(payload, jsep);
  let status = response.plugindata.data.status;

  if (status !== 200) {
    console.error(`\`stream.create\`: ${status}`);
    return;
  }

  // Set JSEP from the response as remote description.
  let sdpAnswer = new RTCSessionDescription(response.jsep);

  if (sdpAnswer.type !== 'answer') {
    console.error(`Bad SDP type. Expected answer, got ${sdpAnswer.type}`);
    return;
  }

  peerConnection.setRemoteDescription(sdpAnswer);
}
