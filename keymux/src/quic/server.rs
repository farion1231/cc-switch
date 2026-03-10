use super::engine::{QuicEngine, Role};
use super::error::QuicError;
use super::protocol::*;
use parking_lot::Mutex;
use socket2::{Domain, Socket, Type};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

pub struct QuicServer {
    socket: Arc<UdpSocket>,
    connections: Arc<Mutex<HashMap<SocketAddr, Arc<QuicEngine>>>>,
}

impl QuicServer {
    pub async fn bind(addr: SocketAddr) -> Result<Self, QuicError> {
        let socket = Socket::new(Domain::for_address(addr), Type::DGRAM, None).map_err(QuicError::Io)?;

        socket.set_reuse_address(true).map_err(QuicError::Io)?;
        #[cfg(unix)]
        socket.set_reuse_port(true).map_err(QuicError::Io)?;

        socket.bind(&addr.into()).map_err(QuicError::Io)?;

        let std_socket: std::net::UdpSocket = socket.into();
        std_socket.set_nonblocking(true).map_err(QuicError::Io)?;

        let tokio_socket = UdpSocket::from_std(std_socket).map_err(QuicError::Io)?;

        Ok(Self {
            socket: Arc::new(tokio_socket),
            connections: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn start(&self) -> Result<(), QuicError> {
        let socket = self.socket.clone();
        let connections = self.connections.clone();

        tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];
            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((len, remote_addr)) => {
                        let packet_data = &buf[..len];

                        match deserialize_packet(packet_data) {
                            Ok(received_packet) => {
                                let engine_arc = {
                                    let mut connections_guard = connections.lock();
                                    connections_guard.entry(remote_addr).or_insert_with(|| {
                                        let local_conn_id = ConnectionId {
                                            bytes: rand::random::<[u8; 8]>().to_vec(),
                                        };
                                        let remote_conn_id = ConnectionId {
                                            bytes: rand::random::<[u8; 8]>().to_vec(),
                                        };

                                        let initial_state = QuicConnectionState {
                                            local_connection_id: local_conn_id,
                                            remote_connection_id: remote_conn_id,
                                            version: 1,
                                            transport_params: TransportParameters::default(),
                                            streams: Vec::new(),
                                            sent_packets: Vec::new(),
                                            received_packets: Vec::new(),
                                            next_packet_number: 0,
                                            next_stream_id: 0,
                                            congestion_window: 14720,
                                            bytes_in_flight: 0,
                                            rtt: 100,
                                            connection_state: ConnectionState::Handshaking,
                                        };
                                        Arc::new(QuicEngine::new(
                                            Role::Server,
                                            initial_state,
                                            socket.clone(),
                                            remote_addr,
                                            vec![0; 32],
                                        ))
                                    }).clone()
                                };

                                match engine_arc.process_packet(received_packet.clone()).await {
                                    Ok(()) => {
                                        for frame in received_packet.frames.iter() {
                                            if let QuicFrame::Stream(stream_frame) = frame {
                                                tracing::info!(
                                                    "Server received stream data on stream {}: {:?}",
                                                    stream_frame.stream_id,
                                                    stream_frame.data
                                                );
                                                if let Err(e) = engine_arc
                                                    .send_stream_data(
                                                        stream_frame.stream_id,
                                                        stream_frame.data.clone(),
                                                    )
                                                    .await
                                                {
                                                    tracing::error!("Failed to echo stream data: {}", e);
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Error processing packet: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to deserialize packet: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("UDP socket receive error: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    pub async fn accept(&self) -> Option<Arc<QuicEngine>> {
        None
    }

    pub async fn close(&self) {
        let connections_guard = self.connections.lock();
        for (_, engine) in connections_guard.iter() {
            engine.close().await;
        }
    }
}
