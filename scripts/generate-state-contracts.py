#!/usr/bin/env python3
"""Generate client-owned typed stored-state messages; field order assigns tags.

Changes require reviewing library_replica::storage::VERSION, independently of
HTTP. The maintenance tool alone interprets the previous generic payloads.
"""
import os
import subprocess
from pathlib import Path
os.chdir(Path(__file__).resolve().parents[1])

structs = {
 'BookFacts': [('added_at','u64'),('format','format')],
 'ReadingPositionState': [('location','position'),('progress','f32')],
 'BookAddition': [('facts','BookFacts'),('record','record'),('navigation','?BookTocDocument'),('pdf','?PdfReaderMetadata'),('recording','?LookupRequest'),('enrichment','?EnrichmentResult')],
 'BookTocEntry': [('title','str'),('target','str'),('children','*BookTocEntry:100000')],
 'BookTocDocument': [('entries','*BookTocEntry:100000'),('duration_ms','?u64'),('tracks','?Tracks')],
 'Tracks': [('items','*AudiobookTrack:32767')],
 'TocEntries': [('items','*BookTocEntry:100000')],
 'AudiobookTrack': [('name','str'),('start_ms','u64'),('end_ms','u64'),('offset','u64'),('length','u64'),('archive_hash','?book')],
 'AudiobookChapter': [('title','str'),('start_ms','u64'),('end_ms','u64')],
 'RecordingEvidence': [('album','?str'),('subtitle','?str'),('narrators','*str:4096'),('asins','*str:4096'),('isbns','*str:4096'),('publishers','*str:4096'),('date','?str'),('recording_release_date','?str'),('recording_release_year','?i32')],
 'LookupRequest': [('title','str'),('authors','*str:4096'),('duration_ms','u64'),('recording','RecordingEvidence'),('chapters','*AudiobookChapter:32767')],
 'ChapterReadStatus': [('completed','*str:100000'),('active','?str')],
 'Annotation': [('id','str'),('book_id','book'),('anchor','AnnotationAnchor'),('exact_text','text'),('style','AnnotationStyle'),('color','u32'),('note','text'),('created_at','u64'),('toc_ordinal','?i64'),('progress','?f32'),('modified_at','u64'),('deleted','bool')],
 'PdfAnnotationAnchor': [('page_index','u32'),('rects','*PdfAnnotationRect:4096'),('fallback_cfi','?str')],
 'PdfAnnotationRect': [('left','f32'),('top','f32'),('width','f32'),('height','f32')],
 'PdfReaderMetadata': [('version','u32'),('book_id','str'),('pages','PageColumns'),('annotations','*PdfStoredAnnotation:16384')],
 'PdfStoredAnnotation': [('page_index','usize'),('index','usize'),('kind','PdfStoredAnnotationKind'),('rects','*Rect:4096'),('color','str'),('text','text'),('note','text')],
 'Rect': [('left','f32'),('top','f32'),('width','f32'),('height','f32')],
}
# Tuple variants use the field name "inner"; all others use named fields.
variants = {
 'BookLifecycleState': {'Present':[], 'Deleted':[('origin_folder_id','?str')], 'Purged':[]},
 'EnrichmentResult': {'Found':[('toc','?TocEntries'),('cover_id','?str')], 'NoMatch':[]},
 'AnnotationAnchor': {'EpubCfi':[('cfi','str')], 'Pdf':[('inner','PdfAnnotationAnchor')], 'Recurring':[('origin','box:AnnotationAnchor'),('scope','RecurringScope')], 'RecurringAliases':[('origin','box:AnnotationAnchor'),('scope','RecurringScope'),('aliases','*str:4096')]},
 'LibraryStateEntry': {
  'DirectoryName':[('dir_id','uuid'),('name','str')],
  'DirectoryParent':[('dir_id','uuid'),('parent_id','uuid')],
  'DirectoryLifecycle':[('dir_id','uuid'),('value','DirectoryLifecycleState')],
  'BookLifecycle':[('book_id','book'),('value','BookLifecycleState')],
  'Placement':[('dir_id','uuid'),('book_id','book'),('present','bool'),('origin_folder_id','?str')],
  'ReadingPosition':[('book_id','book'),('value','ReadingPositionState')],
  'Annotation':[('value','Annotation')],
  'BookAdd':[('book_id','book'),('value','BookAddition')],
  'ChapterReadStatus':[('book_id','book'),('value','ChapterReadStatus')],
  'BookContent':[('book_id','book'),('checksum','hash'),('size_bytes','?u64'),('pdf','?PdfReaderMetadata')],
 }
}
enums={'DirectoryLifecycleState':['Present','Deleted','Purged'],'AnnotationStyle':['Highlight','Underline','Squiggly','Strikethrough'],'RecurringScope':['Chapter','Book','Library'],'PdfStoredAnnotationKind':['Highlight','Underline','Squiggly','Strikethrough','Note']}
prim={'str':('String','string','Bytes(16384)'), 'text':('String','string','Bytes(4 * 1024 * 1024)'), 'position':('String','string','Bytes(16384)'), 'record':('Vec<u8>','bytes','Message(book_model::record_transport::schema)'), 'uuid':('Vec<u8>','bytes','Bytes(16)'), 'book':('Vec<u8>','bytes','Bytes(33)'), 'hash':('Vec<u8>','bytes','Bytes(32)'), 'format':('u32','uint32','Varint(u8::MAX as u64)'), 'f32':('f32','float','Fixed32')}
for t,r,w,m in [('u64','u64','uint64','u64::MAX'),('u32','u32','uint32','u32::MAX as u64'),('u16','u32','uint32','u16::MAX as u64'),('usize','u64','uint64','usize::MAX as u64'),('i32','i32','sint32','u32::MAX as u64'),('i64','i64','int64','u64::MAX'),('bool','bool','bool','1')]:prim[t]=(r,w,f'Varint({m})')
def base(t):return t.removeprefix('box:').lstrip('?*').split(':')[0]
def domain(t):return {'Tracks':'Vec<AudiobookTrack>','TocEntries':'Vec<BookTocEntry>','Rect':'[f32; 4]','PageColumns':'Vec<PdfStoredPage>'}.get(t,t)
def dec(b,e):
 if b in enums:return 'match '+(e[1:-1] if e.startswith('(') else e)+' { '+', '.join(f'{i} => {b}::{v}' for i,v in enumerate(enums[b]))+f', _ => return Err(invalid("unknown {b}")) }}'
 if b=='uuid':return 'DirId::from_slice(&'+e+').map_err(invalid)?'
 if b=='book':return 'BookId::try_from('+e+'.as_slice()).map_err(invalid)?'
 if b=='hash':return 'ContentHash::try_from('+e+'.as_slice()).map_err(invalid)?'
 if b=='format':return 'BookFormat::from_code(i64::from('+e+')).ok_or_else(|| invalid("invalid book format"))?'
 if b=='position':return 'ReadingPosition::parse(&'+e+').map_err(invalid)?'
 if b=='record':return 'book_model::record_transport::decode(&'+e+')?'
 if b in ('u16','usize'):return b+'::try_from('+e+').map_err(invalid)?'
 if b in prim:return e
 return '<'+domain(b)+' as Codec>::from_proto('+e+')?'

def write_value(tag,b,e,explicit=False):
 arg=e[1:-1] if e.startswith('(') and e.endswith(')') else e
 if b in ('str','text','uuid','book','hash'):
  v=e+'.as_bytes()'
  code=f'encoder.bytes({tag}, {v})?;'
  return code if explicit else f'if !{v}.is_empty() {{ {code} }}'
 if b=='position':return f'encoder.display({tag}, {arg})?;'
 if b=='record':return f'encoder.message({tag}, |encoder| record_transport::encode_fields({arg}, encoder))?;'
 if b not in prim and b not in enums:return f'encoder.message({tag}, |encoder| {e}.encode_fields(encoder))?;'
 if b=='f32':
  code=f'encoder.float({tag}, *{e})?;'
  return code if explicit else f'if *{e} != 0.0 {{ {code} }}'
 if b in enums:
  v='match '+arg+' { '+', '.join(f'{b}::{v} => {i}' for i,v in enumerate(enums[b]))+' }'
 elif b=='i32':v=f'(((*{e} as u32) << 1) ^ ((*{e} >> 31) as u32)) as u64'
 else:v=f'*{e} as u64'
 code=f'encoder.varint({tag}, scalar)?;'
 return f'{{ let scalar = {v}; '+(code if explicit else f'if scalar != 0 {{ {code} }}')+' }'

def write_field(tag,t,e):
 b=base(t)
 if t.startswith('*'):return f'for item in {e}.iter() {{ '+write_value(tag,b,'item',True)+' }'
 if t.startswith('?'):return f'if let Some(item) = {e}.as_ref() {{ '+write_value(tag,b,'item',True)+' }'
 if t.startswith('box:'):e=e+'.as_ref()'
 return write_value(tag,b,e)

def decode_field(field,t,e):
 b=base(t)
 if t.startswith('*'):return e+'.into_iter().map(|item| Ok('+dec(b,'item')+')).collect::<Result<_, WireError>>()?'
 if t.startswith('?'):return e+'.map(|item| Ok('+dec(b,'item')+')).transpose()?'
 if t.startswith('box:'):return 'Box::new('+dec(b,'*required('+e+')?')+')'
 return dec(b,'required('+e+')?' if b not in prim and b not in enums else e)

out=['//! Generated by scripts/generate-state-contracts.py.', 'use super::*;', 'use prost::{Message, Oneof};']
proto=['// Generated by scripts/generate-state-contracts.py.', 'syntax = "proto3";', 'package bokheim.state;', '']

# Client-owned columns: IEEE-754 f32 bits and u16 rotations, little-endian.
proto.extend(['message PageColumns {', '  uint32 count = 1;',
 '  bytes widths = 2; // One or count little-endian f32 values.',
 '  bytes heights = 3; // One or count little-endian f32 values.',
 '  bytes rotations = 4; // Zero, one or count little-endian u16 values.', '}', ''])

def message(name,fields,oneof=False):
 out.extend(['#[derive(Clone, PartialEq, Message)]',f'pub(super) struct {name}Proto {{'])
 specs=[];proto.append(f'message {name} {{')
 for tag,(f,t) in enumerate(fields,1):
  b=base(t); repeated=t.startswith('*');optional=t.startswith('?');boxed=t.startswith('box:');msg=b not in prim and b not in enums
  rust,wire,kind=prim[b] if b in prim else ('i32','int32',f'Varint({len(enums[b])-1})') if b in enums else (b+'Proto','message',f'Message({b}Proto::fields)')
  label='repeated' if repeated else 'optional' if optional or msg else ''
  if boxed:rust='Box<'+rust+'>'
  rust=f'Vec<{rust}>' if repeated else f'Option<{rust}>' if optional or msg else rust
  attr=(', '+label if label else '')+(', boxed' if boxed else '')
  out.extend([f'    #[prost({wire}{attr}, tag="{tag}")]',f'    pub {f}: {rust},'])
  count=t.split(':')[-1] if repeated else '1'
  specs.append(f'Field {{ tag: {tag}, kind: Kind::{kind}, count: {count} }}')
  plabel='repeated ' if repeated else 'optional ' if optional else ''
  proto.append(f'  {plabel}{wire if not msg else b} {f} = {tag};')
 out.extend(['}',f'impl Schema for {name}Proto {{ fn fields() -> &\'static [Field] {{ &[ '+', '.join(specs)+' ] } }'])
 proto.extend(['}', ''])

for name,fields in structs.items():
 message(name,fields)
 encoded=[];decoded=[]
 for i,(f,t) in enumerate(fields):
  expr='(&self.'+f+')'
  if name in ('Tracks','TocEntries'):expr='self'
  elif name=='Rect':expr=f'(&self[{i}])'
  elif name=='ChapterReadStatus' and f=='active':expr='(self.active_target())'
  encoded.append(write_field(i+1,t,expr))
  decoded.append(f'{f}: '+decode_field(f,t,'value.'+f))
 construct='Self { '+', '.join(decoded)+' }'
 if name in ('Tracks','TocEntries'):construct=decode_field('items',fields[0][1],'value.items')
 elif name=='Rect':construct='[value.left, value.top, value.width, value.height]'
 elif name=='ChapterReadStatus':construct='Self::from_parts('+decode_field('completed',fields[0][1],'value.completed')+', value.active)'
 out.extend([f'impl Codec for {domain(name)} {{',f'    type Proto = {name}Proto;',"    fn encode_fields(&self, encoder: &mut Encoder<'_>) -> Result<(), WireError> { "+' '.join(encoded)+' Ok(()) }',f'    fn from_proto(value: Self::Proto) -> Result<Self, WireError> {{ Ok({construct}) }}','}'])

for name,cases in variants.items():
 for variant,fields in cases.items():message(name+variant,fields)
 tags=', '.join(str(i) for i in range(1,len(cases)+1))
 out.extend(['#[derive(Clone, PartialEq, Message)]',f'pub(super) struct {name}Proto {{',f'    #[prost(oneof="{name}Kind", tags="{tags}")]',f'    pub kind: Option<{name}Kind>,','}',f'impl Schema for {name}Proto {{'])
 if name=='LibraryStateEntry':out.extend(['    const MAX_FIELDS: usize = 524_288;', '    const MAX_DEPTH: usize = 96;'])
 out.extend(['    fn fields() -> &\'static [Field] { &[ '+', '.join(f'Field {{ tag: {i}, kind: Kind::Oneof({name}{v}Proto::fields), count: 1 }}' for i,v in enumerate(cases,1))+' ] }','}', '#[derive(Clone, PartialEq, Oneof)]',f'pub(super) enum {name}Kind {{'])
 for i,v in enumerate(cases,1):out.extend([f'    #[prost(message, tag="{i}")]',f'    {v}(Box<{name}{v}Proto>),'])
 out.append('}')
 out.extend([f'impl Codec for {name} {{',f'    type Proto = {name}Proto;',"    fn encode_fields(&self, encoder: &mut Encoder<'_>) -> Result<(), WireError> { match self {"])
 for tag,(v,fields) in enumerate(cases.items(),1):
  pat=f'Self::{v}' + ('(inner)' if fields and fields[0][0]=='inner' else ' { '+', '.join(f for f,t in fields)+' }' if fields else '')
  out.append(pat+f' => encoder.message({tag}, |'+('encoder' if fields else '_')+'| { '+' '.join(write_field(i,t,f) for i,(f,t) in enumerate(fields,1))+' Ok(()) }),')
 out.extend(['    } }', '    fn from_proto(value: Self::Proto) -> Result<Self, WireError> { Ok(match required(value.kind)? {'])
 for v,fields in cases.items():
  construct=f'Self::{v}' + ('('+decode_field('inner',fields[0][1],'value.inner')+')' if fields and fields[0][0]=='inner' else ' { '+', '.join(f+': '+decode_field(f,t,'value.'+f) for f,t in fields)+' }' if fields else '')
  out.append(f'        {name}Kind::{v}('+('value' if fields else '_')+') => '+construct+',')
 out.extend(['    }) }','}'])
 proto.extend([f'message {name} {{','  oneof kind {']+[f'    {name}{v} {"".join(("_"+c.lower()) if c.isupper() else c for c in v).lstrip("_")} = {i};' for i,v in enumerate(cases,1)]+['  }','}',''])
Path('client/library-backend/crates/library-replica/src/storage/generated.rs').write_text('\n'.join(out)+'\n')
Path('client/library-backend/crates/library-replica/src/storage/state.proto').write_text('\n'.join(proto)+'\n')
subprocess.run(['rustfmt','--edition','2024','--config','skip_children=true','client/library-backend/crates/library-replica/src/storage/generated.rs'],check=True)
