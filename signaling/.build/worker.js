var __defProp = Object.defineProperty;
var __name = (target, value) => __defProp(target, "name", { value, configurable: true });

// room.ts
var UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
var bytes = /* @__PURE__ */ __name((s) => new TextEncoder().encode(s).length, "bytes");
var object = /* @__PURE__ */ __name((v) => v !== null && typeof v === "object" && !Array.isArray(v), "object");
var Room = class {
  static {
    __name(this, "Room");
  }
  peers = [];
  started = 0;
  finished = false;
  phase = "waiting";
  candidates = 0;
  connectionId;
  now;
  constructor(connectionId, now = () => Date.now()) {
    this.connectionId = connectionId;
    this.now = now;
  }
  send(peer, type, payload, request_id = crypto.randomUUID()) {
    peer.socket.send(JSON.stringify({ v: 1, type, connection_id: this.connectionId, request_id, payload }));
  }
  add(socket) {
    this.expire();
    if (this.finished || this.peers.length >= 2) return false;
    if (!this.peers.length) this.started = this.now();
    this.peers.push({ socket, ended: false });
    return true;
  }
  expire() {
    if (this.peers.length && (this.now() - this.started >= 3e5 || this.phase !== "answered" && this.now() - this.started >= 3e4)) this.finish("timeout");
  }
  finish(reason) {
    this.finished = true;
    const peers = this.peers.splice(0);
    this.phase = "waiting";
    this.candidates = 0;
    for (const peer of peers) {
      try {
        this.send(peer, "close", { reason });
      } catch {
      }
      try {
        peer.socket.close(reason === "protocol_error" ? 1008 : 1e3, reason);
      } catch {
      }
    }
  }
  left(socket) {
    if (this.peers.some((p) => p.socket === socket)) this.finish("peer_left");
  }
  receive(socket, data) {
    this.expire();
    const peer = this.peers.find((p) => p.socket === socket);
    if (!peer) return;
    let requestId;
    try {
      if (typeof data !== "string" || bytes(data) > 65536) throw Error();
      const m = JSON.parse(data);
      if (!object(m) || m.v !== 1 || m.connection_id !== this.connectionId || typeof m.request_id !== "string" || !m.request_id.length || m.request_id.length > 128 || !/^[\w.-]+$/.test(m.request_id) || !object(m.payload)) throw Error();
      requestId = m.request_id;
      const p = m.payload;
      const other = this.peers.find((p2) => p2 !== peer);
      if (m.type === "hello") {
        if (peer.role || p.role !== "mobile" && p.role !== "host" || this.peers.some((x) => x.role === p.role)) throw Error();
        peer.role = p.role;
        if (other?.role) {
          this.phase = "ready";
          for (const target of this.peers) this.send(target, "hello", { status: "ready" }, requestId);
        } else this.send(peer, "hello", { status: "waiting" }, requestId);
        return;
      }
      if (!peer.role) throw Error();
      if (m.type === "close") {
        if (!["done", "timeout", "peer_left", "protocol_error"].includes(p.reason)) throw Error();
        this.finish(p.reason);
        return;
      }
      if (!other?.role) throw Error();
      if (m.type === "offer" || m.type === "answer") {
        if (typeof p.sdp !== "string" || !p.sdp.trim()) throw Error();
        if (m.type === "offer") {
          if (peer.role !== "mobile" || this.phase !== "ready") throw Error();
          this.phase = "offered";
        } else {
          if (peer.role !== "host" || this.phase !== "offered") throw Error();
          this.phase = "answered";
        }
      } else if (m.type === "ice") {
        if (this.phase === "waiting" || this.phase === "ready" || peer.role === "host" && this.phase !== "answered" || peer.ended || typeof p.candidate !== "string" || bytes(p.candidate) > 4096 || !(p.sdpMid === null || typeof p.sdpMid === "string") || !(p.usernameFragment === null || typeof p.usernameFragment === "string") || !(p.sdpMLineIndex === null || Number.isInteger(p.sdpMLineIndex) && p.sdpMLineIndex >= 0 && p.sdpMLineIndex <= 65535)) throw Error();
        if (p.candidate === "") peer.ended = true;
        else if (++this.candidates > 256) throw Error();
      } else throw Error();
      other.socket.send(data);
    } catch {
      try {
        this.send(peer, "error", { code: "INVALID_MESSAGE", message: "Invalid signaling message" }, requestId);
      } catch {
      }
      this.finish("protocol_error");
    }
  }
};

// worker.ts
var worker_default = {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (!["127.0.0.1", "localhost", "[::1]"].includes(url.hostname)) return new Response("Local only", { status: 403 });
    if (!env.ROOM_ID || !UUID.test(env.ROOM_ID) || !env.CONNECTION_ID || !UUID.test(env.CONNECTION_ID)) return new Response("Local configuration required", { status: 503 });
    if (url.pathname !== `/v1/ws/${env.ROOM_ID}` || url.search) return new Response("Not found", { status: 404 });
    if (request.method !== "GET" || request.headers.get("Upgrade")?.toLowerCase() !== "websocket") return new Response("WebSocket required", { status: 426 });
    if (request.headers.has("Origin")) return new Response("Origin denied", { status: 403 });
    return env.ROOM.get(env.ROOM.idFromName(env.ROOM_ID)).fetch(request);
  }
};
var SignalingRoom = class {
  static {
    __name(this, "SignalingRoom");
  }
  room;
  timer;
  state;
  admitted = false;
  unavailable = false;
  constructor(state, env) {
    this.state = state;
    this.room = new Room(env.CONNECTION_ID);
  }
  async fetch() {
    return this.state.blockConcurrencyWhile(async () => {
      if (this.unavailable) return new Response("Storage unavailable", { status: 503 });
      if (!this.admitted) {
        try {
          if (await this.state.storage.get("consumed") !== void 0) return new Response("Room consumed", { status: 409 });
          await this.state.storage.put("consumed", { connectionId: this.room.connectionId });
          this.admitted = true;
        } catch {
          this.unavailable = true;
          return new Response("Storage unavailable", { status: 503 });
        }
      }
      return this.upgrade();
    });
  }
  upgrade() {
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    if (!this.room.add(server)) return new Response("Room full", { status: 409 });
    server.accept();
    const cleanup = /* @__PURE__ */ __name(() => {
      if (!this.room.peers.length && this.timer !== void 0) {
        clearInterval(this.timer);
        this.timer = void 0;
      }
    }, "cleanup");
    server.addEventListener("message", (event) => {
      this.room.receive(server, event.data);
      cleanup();
    });
    for (const type of ["close", "error"]) server.addEventListener(type, () => {
      this.room.left(server);
      cleanup();
    });
    this.timer ??= setInterval(() => {
      this.room.expire();
      cleanup();
    }, 100);
    return new Response(null, { status: 101, webSocket: client });
  }
};
export {
  SignalingRoom,
  worker_default as default
};
//# sourceMappingURL=worker.js.map
