use super::error::QuicError;
use super::protocol::*;
use parking_lot::Mutex;
use rand::Rng;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

pub enum Role {
    Client,
    Server,
}

pub struct QuicEngine {
    role: Role,
    state: Arc<Mutex<QuicConnectionState>>,
    stream_states: Arc<Mutex<HashMap<u64, QuicStreamState>>>,
    ack_pending: Arc<Mutex<Vec<u64>>>,
    socket: Arc<UdpSocket>,
    remote_addr: SocketAddr,
}

impl QuicEngine {
    pub fn new(
        role: Role,
        initial_state: QuicConnectionState,
        socket: Arc<UdpSocket>,
        remote_addr: SocketAddr,
        _private_key: Vec<u8>,
    ) -> Self {
        let mut state = initial_state;
        state.connection_state = ConnectionState::Handshaking;
        QuicEngine {
            role,
            state: Arc::new(Mutex::new(state)),
            stream_states: Arc::new(Mutex::new(HashMap::new())),
            ack_pending: Arc::new(Mutex::new(Vec::new())),
            socket,
            remote_addr,
        }
    }

    pub async fn process_packet(&self, packet: QuicPacket) -> Result<(), QuicError> {
        let (ack_packet_opt, serialized_ack_opt) = {
            let mut state_guard = self.state.lock();
            let mut stream_states_guard = self.stream_states.lock();
            let mut ack_pending_guard = self.ack_pending.lock();

            for frame in packet.frames.iter() {
                match frame {
                    QuicFrame::Stream(stream_frame) => {
                        self.process_stream_frame(
                            stream_frame,
                            &mut stream_states_guard,
                            &mut ack_pending_guard,
                            &state_guard,
                        )?;
                    }
                    QuicFrame::Ack(ack_frame) => {
                        self.process_ack_frame(ack_frame, &mut state_guard);
                        if state_guard.connection_state == ConnectionState::Handshaking {
                            state_guard.connection_state = ConnectionState::Connected;
                            tracing::info!("Connection state transitioned to Connected.");
                        }
                    }
                    QuicFrame::Crypto(crypto_frame) => {
                        self.process_crypto_frame(crypto_frame, &mut ack_pending_guard, &state_guard)?;
                    }
                    _ => {}
                }
            }

            state_guard.received_packets.push(packet);
            state_guard.next_packet_number += 1;

            if !ack_pending_guard.is_empty() {
                let ack_packet = self.create_ack_packet(&state_guard, &ack_pending_guard)?;
                let serialized_ack = serialize_packet(&ack_packet)?;
                ack_pending_guard.clear();
                (Some(ack_packet), Some(serialized_ack))
            } else {
                (None, None)
            }
        };

        if let Some(serialized_ack) = serialized_ack_opt {
            self.socket
                .send_to(&serialized_ack, self.remote_addr)
                .await
                .map_err(QuicError::Io)?;
        }

        Ok(())
    }

    pub async fn send_stream_data(&self, stream_id: u64, data: Vec<u8>) -> Result<(), QuicError> {
        let serialized_packet = {
            let mut state_guard = self.state.lock();
            let mut stream_states_guard = self.stream_states.lock();

            let stream = stream_states_guard.entry(stream_id).or_insert_with(|| {
                QuicStreamState {
                    stream_id,
                    send_buffer: Vec::new(),
                    receive_buffer: Vec::new(),
                    send_offset: 0,
                    receive_offset: 0,
                    max_data: state_guard.transport_params.max_stream_data,
                    state: StreamState::Idle,
                }
            });

            let frame = StreamFrame {
                stream_id,
                offset: stream.send_offset,
                data: data.clone(),
                fin: false,
            };

            stream.send_buffer.extend_from_slice(&data);
            stream.send_offset += data.len() as u64;

            let packet = QuicPacket {
                header: QuicHeader {
                    r#type: QuicPacketType::ShortHeader,
                    version: state_guard.version,
                    destination_connection_id: state_guard.remote_connection_id.clone(),
                    source_connection_id: state_guard.local_connection_id.clone(),
                    packet_number: state_guard.next_packet_number,
                    token: None,
                },
                frames: vec![QuicFrame::Stream(frame)],
                payload: data.clone(),
            };

            state_guard.sent_packets.push(packet.clone());
            state_guard.next_packet_number += 1;
            state_guard.bytes_in_flight += data.len() as u64;

            serialize_packet(&packet)?
        };

        self.socket
            .send_to(&serialized_packet, self.remote_addr)
            .await
            .map_err(QuicError::Io)?;

        Ok(())
    }

    pub fn create_stream(&self) -> u64 {
        let mut state_guard = self.state.lock();
        let new_stream_id = state_guard.next_stream_id;
        state_guard.next_stream_id += 1;
        new_stream_id
    }

    fn process_stream_frame(
        &self,
        frame: &StreamFrame,
        stream_states_guard: &mut HashMap<u64, QuicStreamState>,
        ack_pending_guard: &mut Vec<u64>,
        state_guard: &QuicConnectionState,
    ) -> Result<(), QuicError> {
        let stream = stream_states_guard.entry(frame.stream_id).or_insert_with(|| {
            QuicStreamState {
                stream_id: frame.stream_id,
                send_buffer: Vec::new(),
                receive_buffer: Vec::new(),
                send_offset: 0,
                receive_offset: 0,
                max_data: state_guard.transport_params.max_stream_data,
                state: StreamState::Idle,
            }
        });

        stream.receive_buffer.extend_from_slice(&frame.data);
        stream.receive_offset = frame.offset + frame.data.len() as u64;

        ack_pending_guard.push(state_guard.next_packet_number.saturating_sub(1));
        Ok(())
    }

    fn process_ack_frame(&self, frame: &AckFrame, state_guard: &mut QuicConnectionState) {
        let mut acked_bytes = 0u64;
        for (start, end) in frame.ack_ranges.iter() {
            acked_bytes += (end - start + 1) * 1350;
        }

        state_guard.bytes_in_flight = state_guard.bytes_in_flight.saturating_sub(acked_bytes);
    }

    fn process_crypto_frame(
        &self,
        _frame: &CryptoFrame,
        ack_pending_guard: &mut Vec<u64>,
        state_guard: &QuicConnectionState,
    ) -> Result<(), QuicError> {
        ack_pending_guard.push(state_guard.next_packet_number.saturating_sub(1));
        Ok(())
    }

    fn create_ack_packet(
        &self,
        state_guard: &QuicConnectionState,
        ack_pending_guard: &[u64],
    ) -> Result<QuicPacket, QuicError> {
        let mut sorted_acks = ack_pending_guard.to_owned();
        sorted_acks.sort_unstable();

        let mut ranges = Vec::new();
        if !sorted_acks.is_empty() {
            let mut start = sorted_acks[0];
            let mut end = sorted_acks[0];

            for &v in sorted_acks.iter().skip(1) {
                if v == end + 1 {
                    end = v;
                } else {
                    ranges.push((start, end));
                    start = v;
                    end = v;
                }
            }
            ranges.push((start, end));
        }

        let ack_frame = AckFrame {
            largest_acknowledged: *sorted_acks.last().unwrap_or(&0),
            ack_delay: 0,
            ack_ranges: ranges,
        };

        Ok(QuicPacket {
            header: QuicHeader {
                r#type: QuicPacketType::ShortHeader,
                version: state_guard.version,
                destination_connection_id: state_guard.remote_connection_id.clone(),
                source_connection_id: state_guard.local_connection_id.clone(),
                packet_number: state_guard.next_packet_number,
                token: None,
            },
            frames: vec![QuicFrame::Ack(ack_frame)],
            payload: Vec::new(),
        })
    }

    pub fn get_state(&self) -> QuicConnectionState {
        self.state.lock().clone()
    }

    pub fn set_connection_state(&self, new_state: ConnectionState) {
        let mut s = self.state.lock();
        s.connection_state = new_state;
    }

    pub fn get_stream(&self, stream_id: u64) -> Option<QuicStreamState> {
        self.stream_states.lock().get(&stream_id).cloned()
    }

    pub fn get_active_streams(&self) -> Vec<u64> {
        self.stream_states.lock().keys().cloned().collect()
    }

    pub async fn close(&self) {}
}

pub struct KeyPair {
    pub private: Vec<u8>,
    pub public: Vec<u8>,
}

pub fn generate_key_pair() -> KeyPair {
    let mut rng = rand::thread_rng();
    let private_key: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
    let public_key: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
    KeyPair {
        private: private_key,
        public: public_key,
    }
}
