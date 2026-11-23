#!/usr/bin/env python3
"""Generate HTTP protobuf messages, allocation schemas and borrowed encoders.

Run from any directory. Field order assigns protobuf tags; changing a contract
requires an HTTP protocol bump. Persisted payload versions are independent.
"""
from pathlib import Path
import os
import subprocess

os.chdir(Path(__file__).resolve().parents[1])
# Explicit field numbers are part of the HTTP contract, independent of Serde.
sync = {
'VersionKey': [('changed_at','millis'),('conflict_rank','ConflictRank'),('replica_id','uuid'),('replica_seq','seq')],
'SyncCursor': [('generation','?uuid'),('lifecycle_revision','?revision'),('field_revision','?revision')],
'StateAddress': [('kind','str'),('entity_key','key'),('entity_subkey','str')],
'OpaqueStateEntry': [('address','StateAddress'),('book','?book'),('bytes','bytes')],
'VersionedState': [('value','OpaqueStateEntry'),('version','VersionKey')],
'OpaquePublication': [('publisher_seq','seq'),('state','VersionedState')],
'StateDelivery': [('state','VersionedState'),('revision','revision')],
'PushMutationsRequest': [('library_id','uuid'),('replica_id','uuid'),('mutations','*OpaquePublication:1000')],
'PushMutationsResponse': [('rejected','*MutationRejection:1000')],
'MutationRejection': [('publisher_seq','seq'),('reason','MutationRejectionReason')],
'PullStateResponse': [('mutations','*StateDelivery:4096'),('next_cursor','SyncCursor'),('has_more','bool')],
'PullStateQuery': [('library_id','uuid'),('replica_id','uuid'),('cursor','SyncCursor')],
'SyncExchangeRequest': [('library_id','uuid'),('replica_id','uuid'),('mutations','*OpaquePublication:1000'),('cursor','SyncCursor')],
'SyncExchangeResponse': [('push','PushMutationsResponse'),('pull','PullStateResponse')],
'PlaybackGrant': [('path','str'),('expires_in_seconds','u32'),('book_id','book'),('length','u64')],
'ThumbnailPresenceRequest': [('book_ids','*book:4096')],
'ThumbnailPresenceResponse': [('present','*book:4096'),('revisions','*ThumbnailRevision:4096')],
'ThumbnailRevision': [('book_id','book'),('revision','book')],
'BlobManifestEntry': [('book_id','book'),('size_bytes','u64'),('byte_checksum','?hash')],
'BlobManifestRequest': [('blobs','*BlobManifestEntry:4096')],
'RejectedBlobManifestEntry': [('book_id','book'),('reason','BlobManifestRejectionReason')],
'BlobManifestResponse': [('owned','*book:4096'),('upload','*book:4096'),('rejected','*RejectedBlobManifestEntry:4096')],
'ThumbnailBatchEntry': [('book_id','book'),('bytes','image'),('browse_bytes','image')],
'ThumbnailBatchUploadRequest': [('thumbnails','*ThumbnailBatchEntry:16')],
'ThumbnailBatchDownloadRequest': [('book_ids','*book:16')],
'ThumbnailBatchDownloadResponse': [('thumbnails','*ThumbnailBatchEntry:16'),('missing','*book:16')],
'LibrarySummary': [('library_id','uuid'),('library_name','str'),('storage','LibraryStorage')],
'LibraryNameRequest': [('library_id','uuid'),('library_name','str'),('storage','LibraryStorage')],
'CloudStorageChange': [('change_id','uuid'),('enabled','bool')],
'LibraryCloudStorage': [('library_id','uuid'),('enabled','bool')],
'DeleteLibraryResponse': [('library_id','uuid')],
}
account={
'LoginRequest':[('email','str'),('password','str')],
'DeleteAccountRequest':[('email','str'),('password','str'),('confirmation','str')],
'PublicRegistrationRequest':[('email','str'),('password','str')],
'EmailRequest':[('email','str')],
'EmailVerificationRequest':[('email','str'),('pin','str'),('password','str')],
'ResetPasswordRequest':[('token','str'),('new_password','str')],
'RefreshRequest':[('refresh_token','str')],
'AuthResponse':[('token','str'),('expires_at','i64'),('refresh_token','str'),('refresh_expires_at','i64'),('user_id','str'),('email','str')],
'MeResponse':[('user_id','str'),('email','str')],
'StorageUsageResponse':[('used_bytes','u64'),('reserved_bytes','u64'),('quota_bytes','u64')],
'LibraryStorageUsage':[('library_id','str'),('used_bytes','u64')],
'BookStorageUsage':[('book_id','book'),('used_bytes','u64')],
'AccountSnapshotResponse':[('libraries','*LibrarySummary:4096'),('deleted_library_ids','*uuid:4096'),('storage','StorageUsageResponse'),('library_storage','*LibraryStorageUsage:4096')],
'SessionSummary':[('session_id','str'),('created_at','i64'),('expires_at','i64'),('current','bool')],
'SessionRevocationResponse':[('revoked','bool'),('signed_out','bool')],
'OtherSessionsRevocationResponse':[('revoked','u64')],
'StoragePlan':[('id','str'),('storage_quota_bytes','u64')],
'ScheduledStoragePlanChange':[('plan','StoragePlan'),('effective_at','i64')],
'AccountSubscription':[('plan','StoragePlan'),('pending_plan_change','?ScheduledStoragePlanChange'),('status','SubscriptionStatus'),('current_period_end','?i64'),('cancel_at_period_end','bool'),('cancel_effective_at','?i64'),('access_until','?i64'),('access_active','bool')],
'AccountEntitlementsResponse':[('base_storage_quota_bytes','u64'),('storage_quota_bytes','u64'),('subscription','?AccountSubscription')],
}
enums={'ConflictRank':['Update','Delete','Purge'],'MutationRejectionReason':['TimestampTooFarFuture','InvalidPayload'],'BlobManifestRejectionReason':['InvalidSize','SizeMismatch','QuotaExceeded','UploadInProgress'],'LibraryStorage':['Local','Bokheim'],'SubscriptionStatus':['Pending','Trialing','Active','PastDue','Paused','Canceled','Expired']}
alias={'VersionedState':'Versioned<OpaqueStateEntry>'}
primitives={'str':('String','string','Kind::Bytes(4096)'), 'uuid':('Vec<u8>','bytes','Kind::Bytes(16)'), 'book':('Vec<u8>','bytes','Kind::Bytes(33)'), 'hash':('Vec<u8>','bytes','Kind::Bytes(32)'), 'key':('Vec<u8>','bytes','Kind::Bytes(1024)'), 'bytes':('Vec<u8>','bytes','Kind::Bytes(4 * 1024 * 1024)'), 'image':('Vec<u8>','bytes','Kind::Bytes(4 * 1024 * 1024)')}
for t in ['u64','u32','i64','bool','seq','revision','millis']:
    rust={'seq':'u64','revision':'u64','millis':'u64'}.get(t,t)
    primitives[t]=(rust,{'u64':'uint64','u32':'uint32','i64':'int64','bool':'bool'}[rust],'Kind::Varint(u64::MAX)')

for t, maximum in [('bool','1'), ('u32','u32::MAX as u64'), ('seq','i64::MAX as u64'), ('revision','i64::MAX as u64'), ('millis','i64::MAX as u64')]:
    rust, wire, _ = primitives[t]
    primitives[t] = rust, wire, f'Kind::Varint({maximum})'

def base(t):
    return t.lstrip('?*').split(':')[0]
def proto(t):
    if t in primitives:return primitives[t][0]
    if t in enums:return 'i32'
    if t=='LibrarySummary' and section=='account':return '<LibrarySummary as Contract>::Proto'
    return t+'Proto'
def enc(t,e):
    if t in enums:return 'match '+e+' { '+', '.join(t+'::'+v+' => '+str(i) for i,v in enumerate(enums[t]))+' }'
    if t=='uuid':return e+'.as_bytes().to_vec()'
    if t in ('book','hash'):return e+'.as_bytes().to_vec()'
    if t=='key':return e+'.as_bytes().to_vec()'
    if t in ('seq','revision'):return e+'.get()'
    if t in primitives:return e+'.clone()'
    return e+'.to_proto()'
def dec(t,e):
    if t in enums:return 'match '+e+' { '+', '.join(str(i)+' => '+t+'::'+v for i,v in enumerate(enums[t]))+', _ => return Err(invalid("unknown '+t+'")) }'
    if t=='uuid':return ('sync_common::LibraryId' if section=='account' else 'uuid::Uuid')+'::from_slice(&'+e+').map_err(invalid)?'
    if t=='book':return 'decode_book_id(&'+e+')?'
    if t=='hash':return 'content_address::ContentHash::try_from('+e+'.as_slice()).map_err(invalid)?'
    if t=='key':return 'StateKey::from_storage(&value.kind, &'+e+').map_err(invalid)?'
    if t=='seq':return 'ReplicaSeq::new('+e+').map_err(invalid)?'
    if t=='revision':return 'LibraryRevision::new('+e+').map_err(invalid)?'
    if t=='millis':return '{ let v = '+e+'; if v > i64::MAX as u64 { return Err(invalid("timestamp exceeds database range")); } v }'
    if t in primitives:return e
    return alias.get(t,t)+'::from_proto('+e+')?' if t not in alias else '<'+alias[t]+'>::from_proto('+e+')?'
def borrowed(b, e):
    if b in ('str','uuid','book','hash','key'):return 'bytes',e+'.as_bytes()'
    if b in ('bytes','image'):return 'bytes',e+'.as_slice()'
    if b in ('seq','revision'):return 'varint',e.removeprefix('&')+'.get()'
    if b in enums:return 'varint',enc(b,e)
    if b in primitives:return 'varint','('+e+') as u64'
    return 'message',e

def borrowed_methods(fields):
    lengths=[]; writes=[]
    for tag,(field,t) in enumerate(fields,1):
        b=base(t); container=t.startswith(('?','*'))
        expr='item' if container else '&self.'+field
        if b in ('u64','u32','i64','bool','millis'):expr='*item' if container else 'self.'+field
        kind,expr=borrowed(b,expr)
        # Parenthesize borrowed field access before calling methods.
        if kind=='bytes':expr=expr.replace('&self.', 'self.')
        length=f'encoding::{kind}_len({tag}, {expr})'
        write=f'encoding::write_{kind}({tag}, {expr}, output);'
        if container:
            lengths.append(f'self.{field}.iter().map(|item| {length}).sum::<usize>()')
            writes.append(f'if let Some(item) = &self.{field} {{ {write} }}' if t.startswith('?') else f'for item in &self.{field} {{ {write} }}')
        else:
            lengths.append(length);writes.append(write)
    return ['    fn body_len(&self) -> usize { '+' + '.join(lengths)+' }', '    fn encode_body(&self, output: &mut Vec<u8>) { '+' '.join(writes)+' }']

for section,types,path in [('sync',sync,'shared/sync-common/src/http_contracts.rs'),('account',account,'shared/account-contract/src/http_contracts.rs')]:
    out=['//! Generated by scripts/generate-http-contracts.py. Edit the contract definitions there.','use prost::Message;', 'use sync_common::http::encoding;' if section=='account' else 'use crate::http::encoding;','use sync_common::http::{Contract, Field, Kind, Schema, WireError, invalid, required};' if section=='account' else 'use crate::http::{Contract, Field, Kind, Schema, WireError, invalid, required};','use crate::*;']
    if section=='sync':out+=['use crate::api::{assets::*, libraries::*};','use crate::storage::LibraryStorage;','fn decode_book_id(bytes: &[u8]) -> Result<BookId, WireError> { BookId::try_from(bytes).map_err(invalid) }']
    else:out+=['use sync_common::api::libraries::LibrarySummary;','fn decode_book_id(bytes: &[u8]) -> Result<sync_common::BookId, WireError> { sync_common::BookId::try_from(bytes).map_err(invalid) }']
    for name,fields in types.items():
        out+=['#[derive(Clone, PartialEq, Message)]',f'pub struct {name}Proto {{']
        schemas=[];decode=[]
        # State key conversion must borrow kind before it is moved.
        for tag,(field,t) in enumerate(fields,1):
            b=base(t); repeated=t.startswith('*'); optional=t.startswith('?'); message=b not in primitives and b not in enums
            typ=proto(b);label='repeated' if repeated else 'optional' if optional or message else ''
            typ=f'Vec<{typ}>' if repeated else f'Option<{typ}>' if optional or message else typ
            wire=primitives[b][1] if b in primitives else 'int32' if b in enums else 'message'
            extra=', '+label if label else ''
            out += [f'    #[prost({wire}{extra}, tag = "{tag}")]',f'    pub {field}: {typ},']
            kind=primitives[b][2] if b in primitives else f'Kind::Varint({len(enums[b])-1})' if b in enums else f'Kind::Message(<{proto(b)} as Schema>::fields)'
            count=t.split(':')[1] if repeated else '1'
            schemas += [f'Field {{ tag: {tag}, kind: {kind}, count: {count} }}']
            d=dec(b,'item') if repeated or optional else dec(b,('required(value.'+field+')?' if message else 'value.'+field))
            if repeated:d=f'value.{field}.into_iter().map(|item| Ok({d})).collect::<Result<_, WireError>>()?'
            elif optional:d=f'value.{field}.map(|item| Ok({d})).transpose()?'
            decode += [f'{field}: {d}']
        if name=='StateAddress':decode.insert(0,decode.pop(1))
        out+=['}',f'impl Schema for {name}Proto {{',"    fn fields() -> &'static [Field] {",'        &['+', '.join(schemas)+']','    }','}',f'impl Contract for {alias.get(name,name)} {{',f'    type Proto = {name}Proto;',*borrowed_methods(fields),f'    fn from_proto(value: Self::Proto) -> Result<Self, WireError> {{ Ok(Self {{ '+', '.join(decode)+' }) }','}']
    Path(path).write_text('\n'.join(out)+'\n')

# Publish standard protobuf schemas for non-Rust implementations. The bounds in
# the Rust Schema implementations are mandatory admission rules, not proto hints.
for section, types in [('sync', sync), ('account', account)]:
    lines = ['// Generated by scripts/generate-http-contracts.py.', 'syntax = "proto3";', 'package bokheim.http;', '']
    if section == 'account': lines += ['import "sync.proto";', '']
    for name, fields in types.items():
        lines.append(f'message {name} {{')
        for tag, (field, t) in enumerate(fields, 1):
            b = base(t)
            wire = primitives[b][1] if b in primitives else 'int32' if b in enums else b
            label = 'repeated ' if t.startswith('*') else 'optional ' if t.startswith('?') else ''
            if b in enums: lines.append('  // ' + ', '.join(f'{i} = {v}' for i,v in enumerate(enums[b])))
            if t.startswith('*'): lines.append(f'  // Maximum entries: {t.split(":")[1]}.')
            lines.append(f'  {label}{wire} {field} = {tag};')
        lines += ['}', '']
    if section == 'sync':
        lines += ['message Notification {', '  uint32 kind = 1; // 1=hello, 2=library_changed, 3=libraries_changed, 4=reconcile', '  optional uint64 heartbeat_seconds = 2;', '  optional uint64 timeout_seconds = 3;', '  optional bytes library_id = 4;', '}', '', '// Each HTTP body wraps one endpoint-specific message in payload.', 'message Envelope {', '  uint32 version = 1;', '  bytes payload = 2;', '}']
    Path(f'shared/protobuf-wire/proto/{section}.proto').write_text('\n'.join(lines)+'\n')
subprocess.run(['rustfmt', '--edition', '2021', '--config', 'style_edition=2024,skip_children=true', 'shared/sync-common/src/http_contracts.rs', 'shared/account-contract/src/http_contracts.rs'], check=True)
