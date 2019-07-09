use futures::{Future, Stream};
use futures::Sink;
use crossbeam_channel::{Sender, Receiver};
use crossbeam_channel::unbounded;
use warp::ws::WebSocket;
use warp::Filter;
use warp::ws::Message;

static JSON: &str = r#"[ {
  "description": "deno instance",
  "devtoolsFrontendUrl": "chrome-devtools://devtools/bundled/js_app.html?experiments=true&v8only=true&ws=127.0.0.1:9229/websocket",
  "devtoolsFrontendUrlCompat": "chrome-devtools://devtools/bundled/inspector.html?experiments=true&v8only=true&ws=127.0.0.1:9229/websocket",
  "faviconUrl": "https://www.deno-play.app/images/deno.svg",
  "id": "9999-622c-48ac-afec-0974f0f1d378",
  "title": "deno[25591]",
  "type": "deno",
  "url": "file://",
  "webSocketDebuggerUrl": "ws://127.0.0.1:9229/websocket"
} ]"#;

static VERSION: &str = "{ \"Browser\": \"node.js/v10.15.3\", \"Protocol-Version\": \"1.1\", \"webSocketDebuggerUrl\": \"ws://127.0.0.1:3012/9999-622c-48ac-afec-0974f0f1d378\" }";

pub struct Inspector {
  inbound_tx: Sender<String>,
  pub inbound_rx: Receiver<String>,
  pub outbound_tx: Sender<String>,
  outbound_rx: Receiver<String>,
  ready_tx: Sender<()>,
  ready_rx: Receiver<()>,
}

impl Inspector {
  pub fn new() -> Self {

    let (inbound_tx, inbound_rx) = unbounded::<String>();
    let (outbound_tx, outbound_rx) = unbounded::<String>();
    let (ready_tx, ready_rx) = unbounded::<()>();

    Inspector {
      inbound_tx,
      inbound_rx,
      outbound_tx,
      outbound_rx,
      ready_rx,
      ready_tx,
    }
  }

  pub fn start(&mut self, wait: bool) {

    tokio::spawn(self.serve());

    println!("Inspector listening on http://localhost:8595 ... blah");

    if wait {
      self.ready_rx.recv().unwrap();
    }

    println!("Inspector client attached...");
  }

  pub fn serve(&self) -> impl Future<Item = (), Error = ()> {

    let inbound_tx = self.inbound_tx.clone();
    let outbound_rx = self.outbound_rx.clone();
    let ready_tx = self.ready_tx.clone();

    let websocket = warp::path("websocket")
      .and(warp::ws2())
      .map(move |ws: warp::ws::Ws2| {

          let sender = inbound_tx.clone();
          let receiver = outbound_rx.clone();
          let ready_tx = ready_tx.clone();

          ws.on_upgrade(move |socket| {

            std::thread::spawn(move || {
              std::thread::sleep(std::time::Duration::from_secs(5));
              ready_tx.send(()).unwrap();
            });
            on_connection(socket, sender, receiver)
          })
      });

    let json = warp::path("json").map(|| warp::reply::html(JSON));

    let version = path!("json" / "version").map(|| warp::reply::html(VERSION));

    let routes = websocket
        .or(json)
        .or(version);

    warp::serve(routes).bind(([127, 0, 0, 1], 9229))
  }
}

fn on_connection(ws: WebSocket, sender: Sender<String>, receiver: Receiver<String>) -> impl Future<Item = (), Error = ()> {

  let (mut user_ws_tx, user_ws_rx) = ws.split();

  let fut_rx = user_ws_rx
      .for_each(move |msg| {
          let msg_str = msg.to_str().unwrap();
          // println!("FE->RUST: {}", msg_str);
          sender.send(msg_str.to_owned()).unwrap_or_else(|err| println!("Err: {}", err));

          Ok(())
      })
      .map_err(|_|{});

  std::thread::spawn(move || {
    loop {
      let msg = receiver.recv().unwrap();
      // println!("RUST->FE: {}", msg);
      user_ws_tx = user_ws_tx.send(Message::text(msg)).wait().unwrap();
    }
  });

  fut_rx
}
