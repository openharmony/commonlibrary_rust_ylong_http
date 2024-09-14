// Copyright (c) 2023 Huawei Device Co., Ltd.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::h3::parts::Parts;
use crate::h3::qpack::error::ErrorCode::DecoderStreamError;
use crate::h3::qpack::error::{ErrorCode, QpackError};
use crate::h3::qpack::format::encoder::{
    DecInstDecoder, InstDecodeState, PartsIter, ReprEncodeState, SetCap,
};
use crate::h3::qpack::format::ReprEncoder;
use crate::h3::qpack::integer::{Integer, IntegerEncoder};
use crate::h3::qpack::table::{DynamicTable, NameField};
use crate::h3::qpack::{DecoderInstruction, PrefixMask};

pub struct QpackEncoder {
    max_blocked_streams: usize,
    blocked_stream_nums: usize,
    capacity_to_update: Option<usize>,
    tracked_stream: HashMap<u64, UnackFields>,
    table: DynamicTable,
    is_huffman: bool,
    // Headers to be encode.
    field_iter: Option<PartsIter>,
    // save the state of encoding field.
    field_state: Option<ReprEncodeState>,
    // save the state of decoding instructions.
    inst_state: Option<InstDecodeState>,
    insert_length: usize,
    // `RFC`: the number of insertions that the decoder needs to receive before it can decode the
    // field section.
    required_insert_count: usize,
}

#[derive(Default)]
pub(crate) struct UnackFields {
    unacked_section: VecDeque<HashSet<usize>>,
}

impl UnackFields {
    pub(crate) fn new(unacked: VecDeque<HashSet<usize>>) -> Self {
        Self {
            unacked_section: unacked,
        }
    }
    pub(crate) fn max_unacked_index(&self) -> Option<usize> {
        self.unacked_section.iter().flatten().max().cloned()
    }

    pub(crate) fn unacked_section_mut(&mut self) -> &mut VecDeque<HashSet<usize>> {
        &mut self.unacked_section
    }

    pub(crate) fn update(&mut self, unacked: HashSet<usize>) {
        self.unacked_section.push_back(unacked);
    }
}

pub struct EncodeMessage {
    fields: Vec<u8>,
    inst: Vec<u8>,
}

impl EncodeMessage {
    pub fn new(fields: Vec<u8>, inst: Vec<u8>) -> Self {
        Self { fields, inst }
    }
    pub fn fields(&self) -> &Vec<u8> {
        &self.fields
    }

    pub fn inst(&self) -> &Vec<u8> {
        &self.inst
    }
}

impl QpackEncoder {
    pub(crate) fn finish_stream(&self, id: u64) -> Result<(), QpackError> {
        if self.tracked_stream.contains_key(&id) {
            Err(QpackError::ConnectionError(ErrorCode::EncoderStreamError))
        } else {
            Ok(())
        }
    }

    pub(crate) fn set_max_table_capacity(&mut self, max_cap: usize) -> Result<(), QpackError> {
        const MAX_TABLE_CAPACITY: usize = (1 << 30) - 1;
        if max_cap > MAX_TABLE_CAPACITY {
            return Err(QpackError::ConnectionError(ErrorCode::H3SettingsError));
        }
        self.capacity_to_update = Some(max_cap);
        Ok(())
    }

    pub(crate) fn set_max_blocked_stream_size(&mut self, max_blocked: usize) {
        self.max_blocked_streams = max_blocked;
    }

    fn update_max_dynamic_table_cap(&mut self, encoder_buf: &mut Vec<u8>) {
        if let Some(new_cap) = self.capacity_to_update {
            if self.table.update_capacity(new_cap).is_some() {
                SetCap::new(new_cap).encode(encoder_buf);
                self.capacity_to_update = None;
            }
        }
    }

    pub fn set_parts(&mut self, parts: Parts) {
        self.field_iter = Some(PartsIter::new(parts));
    }

    fn ack(&mut self, stream_id: usize) -> Result<(), QpackError> {
        let mut known_received = self.table.known_recved_count();
        if let Some(unacked) = self.tracked_stream.get_mut(&(stream_id as u64)) {
            if let Some(unacked_index) = unacked.unacked_section_mut().pop_front() {
                for index in unacked_index {
                    if (index as u64) > known_received {
                        known_received += 1;
                    }
                    self.table.untracked_field(index);
                }
            }
            if unacked.unacked_section_mut().is_empty() {
                self.tracked_stream.remove(&(stream_id as u64));
            }
        }
        let increment = known_received - self.table.known_recved_count();

        if increment > 0 {
            self.increase_insert_count(increment as usize)
        }
        Ok(())
    }

    fn increase_insert_count(&mut self, increment: usize) {
        self.table.increase_known_receive_count(increment);
        self.update_blocked_stream();
    }

    fn cancel_stream(&mut self, stream_id: u64) {
        let mut stream_blocked = false;
        if let Some(mut fields) = self.tracked_stream.remove(&stream_id) {
            fields
                .unacked_section_mut()
                .iter()
                .flatten()
                .for_each(|index| {
                    self.table.untracked_field(*index);
                    if *index > (self.table.known_recved_count() as usize) {
                        stream_blocked = true;
                    }
                })
        }
        if stream_blocked {
            self.blocked_stream_nums -= 1;
        }
    }

    fn update_blocked_stream(&mut self) {
        let known_receive_cnt = self.table.known_recved_count() as usize;
        let mut blocked = 0;
        self.tracked_stream.iter_mut().for_each(|(_, fields)| {
            if fields
                .unacked_section_mut()
                .iter()
                .flatten()
                .any(|index| *index > known_receive_cnt)
            {
                blocked += 1;
            }
        });
        self.blocked_stream_nums = blocked;
    }

    pub fn decode_ins(&mut self, buf: &[u8]) -> Result<(), QpackError> {
        let mut decoder = DecInstDecoder::new(buf);
        loop {
            match decoder.decode(&mut self.inst_state)? {
                Some(DecoderInstruction::Ack { stream_id }) => self.ack(stream_id)?,
                Some(DecoderInstruction::StreamCancel { stream_id }) => {
                    self.cancel_stream(stream_id as u64);
                }
                Some(DecoderInstruction::InsertCountIncrement { increment }) => {
                    self.increase_insert_count(increment);
                }
                None => return Ok(()),
            }
        }
    }

    pub fn encode(&mut self, stream_id: u64) -> EncodeMessage {
        let mut fields = Vec::new();
        let mut inst = Vec::new();
        self.update_max_dynamic_table_cap(&mut inst);

        let stream_blocked = self
            .tracked_stream
            .get(&stream_id)
            .map_or(false, |unacked| {
                unacked
                    .max_unacked_index()
                    .map_or(false, |idx| (idx as u64) > self.table.known_recved_count())
            });

        let reach_max_block = self.reach_max_blocked();
        let mut encoder = ReprEncoder::new(
            stream_id,
            self.table.insert_count() as u64,
            self.is_huffman,
            &mut self.table,
        );
        encoder.iterate_encode_fields(
            &mut self.field_iter,
            &mut self.tracked_stream,
            &mut self.blocked_stream_nums,
            stream_blocked || !reach_max_block,
            &mut fields,
            &mut inst,
        );
        EncodeMessage::new(fields, inst)
    }

    pub(crate) fn reach_max_blocked(&self) -> bool {
        self.blocked_stream_nums >= self.max_blocked_streams
    }
}

impl Default for QpackEncoder {
    fn default() -> Self {
        Self {
            max_blocked_streams: 0,
            blocked_stream_nums: 0,
            table: DynamicTable::with_empty(),
            tracked_stream: HashMap::new(),
            field_iter: None,
            field_state: None,
            inst_state: None,
            insert_length: 0,
            required_insert_count: 0,
            capacity_to_update: None,
            is_huffman: true,
        }
    }
}

pub(crate) enum DecoderInst {
    Ack,
    StreamCancel,
    InsertCountIncrement,
}
