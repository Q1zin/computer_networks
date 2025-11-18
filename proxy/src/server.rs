use crate::{
    connection::{Connection, EndpointKind},
    handler::{handle_readable, handle_writable},
    util::cleanup_connection,
};
use log::{info, warn};
use mio::{net::TcpListener, Events, Interest, Poll, Token};
use std::{
    collections::HashMap,
    io::{ErrorKind, Result},
};

const SERVER: Token = Token(0);

pub struct Server {
    listener: TcpListener,
    poll: Poll,
    connections: HashMap<usize, Connection>,
    token_map: HashMap<Token, (usize, EndpointKind)>,
    next_connection_id: usize,
    next_token: usize,
}

impl Server {
    pub fn new(port: u16) -> Result<Self> {
        let addr = format!("0.0.0.0:{}", port).parse().unwrap();
        let mut listener = TcpListener::bind(addr)?;
        let poll = Poll::new()?;

        poll.registry()
            .register(&mut listener, SERVER, Interest::READABLE)?;

        println!("SOCKS5 proxy listening on {}", addr);

        Ok(Server {
            listener,
            poll,
            connections: HashMap::new(),
            token_map: HashMap::new(),
            next_connection_id: 1,
            next_token: 1,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        let mut events = Events::with_capacity(1024);

        loop {
            self.poll.poll(&mut events, None)?;

            for event in events.iter() {
                match event.token() {
                    SERVER => self.accept_connections()?,
                    token => self.handle_connection_event(token, event)?,
                }
            }
        }
    }

    fn accept_connections(&mut self) -> Result<()> {
        loop {
            match self.listener.accept() {
                Ok((mut client, client_addr)) => {
                    let client_token = Token(self.next_token);
                    let conn_id = self.next_connection_id;
                    self.next_token += 1;
                    self.next_connection_id += 1;

                    self.poll
                        .registry()
                        .register(&mut client, client_token, Interest::READABLE)?;

                    self.token_map
                        .insert(client_token, (conn_id, EndpointKind::Client));
                    self.connections
                        .insert(conn_id, Connection::new(client, client_token, client_addr));

                    info!(
                        "[conn {conn_id}] New client {client_addr} (token {:?})",
                        client_token
                    );
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn handle_connection_event(&mut self, token: Token, event: &mio::event::Event) -> Result<()> {
        if let Some(&(conn_id, endpoint)) = self.token_map.get(&token) {
            if let Some(conn) = self.connections.get_mut(&conn_id) {
                let mut close = false;

                if event.is_readable() {
                    if handle_readable(
                        conn,
                        conn_id,
                        endpoint,
                        self.poll.registry(),
                        &mut self.token_map,
                        &mut self.next_token,
                    )
                    .is_err()
                    {
                        close = true;
                    }
                }

                if event.is_writable() {
                    if handle_writable(conn, endpoint, self.poll.registry()).is_err() {
                        close = true;
                    }
                }

                if conn.should_close() {
                    close = true;
                }

                if close {
                    cleanup_connection(
                        conn_id,
                        self.poll.registry(),
                        &mut self.connections,
                        &mut self.token_map,
                    )?;
                }
            } else {
                warn!("No connection found for conn_id {}", conn_id);
            }
        } else {
            warn!("No connection found for token {:?}", token);
        }
        Ok(())
    }
}
