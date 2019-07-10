const server = async () => {
  const listener = Deno.listen("tcp", ":8080");
  for (;;) {
    const conn = await listener.accept();
    const buf = new Uint8Array(1024);
    const read = await Deno.read(conn.rid, buf);
    debugger;
  }
};

server();
