use crate::git::{
    any_git_object::{AnyGitObject, Sha},
    commits::Commit,
    compression::decompress_slice,
    git_blob::{Blob, BlobContent},
    git_object_trait::GitObject,
    git_tree::{FileMode, Tree},
};
use anyhow::{anyhow, bail, Context, Result};
use bytes::Bytes;
use reqwest::{Client, Response, Url};
use std::{collections::HashMap, fmt::Debug, path::Path};
use strum::EnumTryAs;
use tokio;
use url::ParseError;

pub struct GitClient {
    url: Url,
    client: Client,
}

impl GitClient {
    pub fn new(url: &str) -> Result<Self> {
        let url = if url.ends_with(".git/") {
            url.to_string()
        } else if url.ends_with(".git") {
            format!("{}/", url)
        } else if url.ends_with("/") {
            format!("{}.git/", &url[..url.len() - 1])
        } else {
            format!("{}.git/", url)
        };

        let url = if url.ends_with("/") {
            url
        } else {
            format!("{}/", url)
        };

        let url = Url::parse(&url)
            .map_err(|err| anyhow!(err).context(format!("failed to create GitClient")))?;

        Ok(Self {
            url,
            client: Client::new(),
        })
    }

    async fn send_pkt_line_request<T: IntoIterator<Item = PktLine>>(
        &self,
        content: T,
        last_pkt_line: Option<PktLine>,
    ) -> Result<Response> {
        let url = self
            .url
            .join("git-upload-pack")
            .with_context(|| "send_pkt_line_request failed: failed to get upload pack URL")?;

        let content = content
            .into_iter()
            .chain(std::iter::once(last_pkt_line.unwrap_or(PktLine::FlushPkt)))
            .map(|line| line.to_bytes())
            .flatten()
            .collect::<Vec<_>>();

        let response = self
            .client
            .post(url)
            .header("Content-Type", UPLOAD_PACK_CONTENT_TYPE)
            .body(content)
            .send()
            .await
            .with_context(|| "failed to send request")?;
        Ok(response)
    }

    async fn send_want_request(
        &self,
        wants: Vec<WantPkt>,
        haves: Option<Vec<HavePkt>>,
        capabilities: Option<GitCapabilities>,
        is_done: bool,
    ) -> Result<Bytes> {
        let mut wants = wants.into_iter();

        let first_want = wants.next().ok_or_else(|| {
            anyhow!("send_want_request failed: wants must contain at least one element")
        })?;

        let first_line = if let Some(GitCapabilities(capabilities)) = capabilities {
            PktLine::StringDataPkt(format!(
                "{} {}",
                first_want.to_string(),
                capabilities.join(" ")
            ))
        } else {
            first_want.to_pkt_line()
        };

        let content = std::iter::once(first_line)
            .chain(wants.map(|want| want.to_pkt_line()))
            .chain(std::iter::once(PktLine::FlushPkt))
            .chain(
                haves
                    .map(|haves| {
                        haves
                            .into_iter()
                            .map(|have| have.to_pkt_line())
                            .chain(std::iter::once(PktLine::FlushPkt))
                    })
                    .into_iter()
                    .flatten(),
            )
            .collect::<Vec<_>>();
        let last_pkt_line = if is_done {
            Some(PktLine::StringDataPkt("done".to_string()))
        } else {
            None
        };

        let response = self
            .send_pkt_line_request(content, last_pkt_line)
            .await
            .with_context(|| "send_want_request failed: failed to send pkt line request")?;

        let response = response
            .error_for_status()
            .with_context(|| "send_want_request failed: HTTP status")?;

        response
            .bytes()
            .await
            .with_context(|| "send_want_request failed: failed to get response bytes")
    }

    pub async fn clone<P: AsRef<Path>>(&self, path: &P) -> Result<()> {
        let ref_discovery = self
            .ref_discovery()
            .await
            .with_context(|| "GitClient::clone: failed to fetch refs")?;

        let mut want_response = self
            .send_want_request(
                vec![WantPkt {
                    object_id: ref_discovery.head_object_id.clone(),
                }],
                None,
                None,
                true,
            )
            .await
            .with_context(|| "GitClient::clone: failed to send want request")?
            .into_iter();

        let line = PktLine::read(want_response.by_ref())
            .with_context(|| "GitClient::clone: failed to read pkt line")?;

        // seems like the server sends NAK if there are no common objects, which will always be the
        // case during a clone operation: https://git-scm.com/docs/pack-protocol#_packfile_negotiation
        assert!(matches!(line, PktLine::StringDataPkt(str) if str == "NAK"));
        let packfile = Packfile::read(want_response.collect::<Vec<_>>())
            .with_context(|| "GitClient::clone: failed to read packfile")?;

        // TODO: validate checksum
        let (deltas, git_objects): (Vec<_>, Vec<_>) =
            packfile.chunks.into_iter().partition(|chunk| match chunk {
                PackfileObject::ObjRefDelta { .. } => true,
                PackfileObject::Blob(_) | PackfileObject::Commit(_) | PackfileObject::Tree(_) => {
                    false
                }
            });

        let mut object_map = git_objects
          .into_iter()
          .map(|chunk| {
              (|| -> Result<_> {
                  Ok(match chunk {
                      PackfileObject::Commit(commit) => {
                          (commit.sha1()?, AnyGitObject::Commit(commit))
                      }
                      PackfileObject::Tree(tree) => (tree.sha1()?, AnyGitObject::Tree(tree)),
                      PackfileObject::Blob(blob) => (blob.sha1()?, AnyGitObject::Blob(blob)),
                      other => unreachable!("GitClient::clone: unexpected object type: git_objects should onlt contain git objects, but got {other:?}"),
                  })
              })()
              .with_context(|| "GitClient::clone: failed to compute sha for git object")
          })
          .collect::<Result<HashMap<_, _>>>()
          .with_context(|| "GitClient::clone: failed to create object map")?;

        let deltas = deltas.into_iter().map(|obj| match obj {
          PackfileObject::ObjRefDelta(delta) => (delta.obj_name.clone(), delta),
          other => unreachable!("GitClient::clone: unexpected object type: deltas should only contain deltas, but got {other:?}"),
      });

        // println!("\n\nApplying deltas");
        for (obj_name, delta) in deltas {
            let obj: &AnyGitObject = object_map.get(&obj_name).ok_or_else(|| {
                anyhow!("GitClient::clone: failed to find object with name {obj_name:?}")
            })?;

            let encoded_obj = obj
                .encode_body()
                .with_context(|| "GitClient::clone: failed to encode object body")?;

            assert_eq!(
                encoded_obj.len(),
                delta.base_obj_size,
                "GitClient::clone: object size doesn't match delta base object size"
            );

            let output = DeltaInstruction::apply(&delta.instructions, &encoded_obj);

            let new_obj = match obj {
                AnyGitObject::Commit(_) => Commit::decode_body(output).map(AnyGitObject::Commit),
                AnyGitObject::Tree(_) => Tree::decode_body(output).map(AnyGitObject::Tree),
                AnyGitObject::Blob(_) => Blob::decode_body(output).map(AnyGitObject::Blob),
            }
            .with_context(|| "GitClient::clone: failed to decode object after delta")?;

            assert_eq!(
                new_obj.encode_body()?.len(),
                delta.target_obj_size,
                "GitClient::clone: object size doesn't match delta target object size"
            );

            object_map.insert(
                new_obj.sha1().with_context(|| {
                    "GitClient::clone: failed to compute sha for object after delta"
                })?,
                new_obj,
            );
        }

        let head = object_map
            .get(&ref_discovery.head_object_id)
            .ok_or_else(|| {
                anyhow!(
                    "GitClient::clone: failed to find HEAD object with SHA {:?}",
                    ref_discovery.head_object_id
                )
            })?
            .try_as_commit_ref()
            .ok_or_else(|| {
                anyhow!(
                    "GitClient::clone: expected HEAD object to be a commit, but got {:?}",
                    object_map.get(&ref_discovery.head_object_id)
                )
            })?;

        let tree = object_map
            .get(&head.tree_hash)
            .ok_or_else(|| {
                anyhow!(
                    "GitClient::clone: failed to find tree object with SHA {:?}",
                    head.tree_hash
                )
            })?
            .try_as_tree_ref()
            .ok_or_else(|| {
                anyhow!(
                    "GitClient::clone: expected tree object to be a tree, but got {:?}",
                    object_map.get(&head.tree_hash)
                )
            })?;

        tokio::fs::create_dir(&path.as_ref().join(".git"))
            .await
            .with_context(|| "GitClient::clone: failed to create .git directory")?;

        for obj in object_map.values() {
            obj.write(&path).with_context(|| {
                format!("GitClient::clone: failed to write object to filesystem {obj:#?}")
            })?;
        }

        ref_discovery
            .write(&path)
            .await
            .with_context(|| "GitClient::clone: failed to write ref discovery to filesystem")?;

        GitClient::write_tree(path, tree, &object_map)
            .with_context(|| "GitClient::clone: failed to write tree object to filesystem")?;

        Ok(())
    }

    fn write_tree<P: AsRef<Path> + ?Sized>(
        path: &P,
        tree: &Tree,
        object_map: &HashMap<Sha, AnyGitObject>,
    ) -> Result<()> {
        let path = path.as_ref();
        for entry in tree.entries() {
            let subpath = path.join(&entry.name);
            match &entry.mode {
                FileMode::Directory => {
                    std::fs::create_dir(&subpath).with_context(|| {
                        format!("GitClient::write_tree: failed to create directory at {path:?}")
                    })?;
                    let subtree = object_map
                      .get(&entry.hash)
                      .ok_or_else(|| {
                          anyhow!(
                              "GitClient::write_tree: failed to find tree object with SHA {:?}",
                              entry.hash
                          )
                      })?
                      .try_as_tree_ref()
                      .ok_or_else(|| {
                          anyhow!(
                              "GitClient::write_tree: expected tree object to be a tree, but got {:?}",
                              object_map.get(&entry.hash)
                          )
                      })?;
                    GitClient::write_tree(&subpath, subtree, object_map).with_context(|| {
                        format!("GitClient::write_tree: failed to write tree object to {subpath:?}")
                    })?;
                }
                FileMode::Regular => {
                    let blob = object_map
                      .get(&entry.hash)
                      .ok_or_else(|| {
                          anyhow!(
                              "GitClient::write_tree: failed to find blob object with SHA {:?}",
                              entry.hash
                          )
                      })?
                      .try_as_blob_ref()
                      .ok_or_else(|| {
                          anyhow!(
                              "GitClient::write_tree: expected blob object to be a blob, but got {:?}",
                              object_map.get(&entry.hash)
                          )
                      })?;
                    std::fs::write(&subpath, blob.content()).with_context(|| {
                        format!("GitClient::write_tree: failed to write blob object to {subpath:?}")
                    })?;
                }

                other => {
                    bail!("GitClient::write_tree: unexpected file mode: {other:?}");
                }
            }
        }
        Ok(())
    }

    async fn ref_discovery(&self) -> Result<GitRefDiscoveryResponse> {
        let url = into_anyhow_result(self.url.join("info/refs").and_then(|mut url| {
            url.set_query(Some("service=git-upload-pack"));
            Ok(url)
        }))
        .with_context(|| "GitClient::ref_discovery: failed to get upload pack URL")?;

        let response = self
            .client
            .get(url)
            .send()
            .await
            .with_context(|| "GitClient::ref_discovery: failed to send request")?
            .error_for_status()
            .with_context(|| "GitClient::ref_discovery: request failed: network")?
            .bytes()
            .await
            .with_context(|| "GitClient::ref_discovery: failed to get response bytes")?;

        let mut iter = PktLine::read_many(response);

        assert!(matches!(
            iter.next(),
            Some(Ok(PktLine::StringDataPkt(str))) if str == "# service=git-upload-pack"
        ));
        assert!(matches!(iter.next(), Some(Ok(PktLine::FlushPkt))));

        let head_line = iter
            .next()
            .ok_or_else(|| anyhow!("expected head line"))??
            .try_as_string_data_pkt()
            .with_context(|| "GitClient::ref_discovery: expected string data pkt")?;

        let mut head_line_chars = head_line.chars().peekable();
        let head = GitRef::read(head_line_chars.by_ref().take_while(|c| c != &'\0'))
            .with_context(|| "GitClient::ref_discovery: failed to parse head ref")?;
        if head.name != "HEAD" {
            bail!("GitClient::ref_discovery: expected HEAD ref in head line");
        }
        let head_object_id = head.object_id;
        let capabilities = GitCapabilities::read(head_line_chars).with_context(|| {
            "GitClient::ref_discovery: failed to parse capabilities in head line"
        })?;
        let refs = iter
            .take_while(|result| !matches!(result, Ok(PktLine::FlushPkt)))
            .map(|result| match result? {
                PktLine::StringDataPkt(str) => GitRef::read(str.chars()),
                _ => bail!("GitClient::ref_discovery: expected string data pkt"),
            })
            .map(|el| el.map(|git_ref| (git_ref.name, git_ref.object_id)))
            .collect::<Result<HashMap<_, _>>>()
            .with_context(|| "GitClient::ref_discovery: failed to parse response")?;
        Ok(GitRefDiscoveryResponse {
            refs,
            head_object_id,
            capabilities,
        })
    }
}

#[derive(Debug)]
struct GitRefDiscoveryResponse {
    refs: HashMap<String, Sha>,
    head_object_id: Sha,
    #[allow(dead_code)]
    capabilities: GitCapabilities,
}

impl GitRefDiscoveryResponse {
    async fn write<P: AsRef<Path>>(&self, path: &P) -> Result<()> {
        let path = path.as_ref().join(".git");
        let head_ref = self
            .refs
            .iter()
            .find(|(_, sha)| sha == &&self.head_object_id)
            .ok_or_else(|| {
                anyhow!("GitRefDiscoveryResponse::write: failed to find HEAD ref in refs")
            })?
            .0;
        tokio::fs::write(&path.join("HEAD"), format!("ref: {head_ref}\n"))
            .await
            .with_context(|| {
                "GitRefDiscoveryResponse::write: failed to write HEAD ref to filesystem"
            })?;
        for (name, object_id) in &self.refs {
            let path = path.join(&name);
            println!("writing ref {name:?} to filesystem {path:?}: {object_id:?}");
            tokio::fs::create_dir_all(path.parent().unwrap())
              .await
              .with_context(|| {
                  format!(
                      "GitRefDiscoveryResponse::write: failed to create parent directories for ref {name:?}: {path:?}"
                  )
              })?;
            tokio::fs::write(path, object_id.to_string())
                .await
                .with_context(|| {
                    format!(
                      "GitRefDiscoveryResponse::write: failed to write ref {name:?} to filesystem"
                  )
                })?;
        }
        Ok(())
    }
}

fn into_anyhow_result<T>(result: Result<T, ParseError>) -> Result<T> {
    result.map_err(|err| anyhow!(err).context("failed to parse URL"))
}

#[derive(Debug, EnumTryAs)]
enum PktLine {
    StringDataPkt(String),
    BinaryDataPkt(Vec<u8>),
    FlushPkt,
}

impl PktLine {
    fn read<T: IntoIterator<Item = u8>>(iter: T) -> Result<Self> {
        let mut iter = iter.into_iter();
        let pkt_len_str = String::from_utf8(iter.by_ref().take(4).collect::<Vec<_>>())
            .with_context(|| "PktLine::read: failed to read pkt-len")?;
        let pkt_len = u64::from_str_radix(&pkt_len_str, 16)
            .with_context(|| format!("PktLine::read: failed to parse pkt-len: {pkt_len_str}"))?;

        if pkt_len == 0 {
            return Ok(Self::FlushPkt);
        } else if pkt_len <= 4 {
            return Err(anyhow!("PktLine::read: pkt-len is too small: {pkt_len}").into());
        }

        let pkt_data = iter
            .take((pkt_len - 4).try_into().with_context(|| {
                format!("PktLine::read: failed to convert pkt-len to usize: {pkt_len}")
            })?)
            .collect::<Vec<_>>();

        if pkt_data.last() == Some(&b'\n') {
            Ok(Self::StringDataPkt(
                String::from_utf8(pkt_data[..pkt_data.len() - 1].to_vec())
                    .with_context(|| "PktLine::read: failed to parse pkt-data as string")?,
            ))
        } else {
            Ok(Self::BinaryDataPkt(pkt_data))
        }
    }

    fn read_many<T: IntoIterator<Item = u8>>(iter: T) -> impl Iterator<Item = Result<Self>> {
        let mut iter = iter.into_iter().peekable();
        std::iter::from_fn(move || {
            if iter.peek().is_some() {
                Some(
                    Self::read(&mut iter)
                        .with_context(|| "PktLine::read_many: failed to read line"),
                )
            } else {
                None
            }
        })
    }

    fn to_bytes(&self) -> Vec<u8> {
        match self {
            PktLine::StringDataPkt(str) => format!("{:04x}{}\n", str.len() + 5, str).into_bytes(),
            PktLine::BinaryDataPkt(data) => {
                let mut pkt = format!("{:04x}", data.len() + 4).into_bytes();
                pkt.extend(data);
                pkt
            }
            PktLine::FlushPkt => b"0000".to_vec(),
        }
    }
}
#[derive(Debug)]
pub struct GitRef {
    object_id: Sha,
    name: String,
}

impl GitRef {
    fn read<T: IntoIterator<Item = char>>(iter: T) -> Result<Self> {
        let mut iter = iter.into_iter();
        let object_id = Sha(hex::decode(
            iter.by_ref()
                .take_while(|&char| char != ' ')
                .collect::<String>(),
        )
        .with_context(|| anyhow!("GitRef::read: failed to decode object_id hex string as bytes"))?
        .try_into()
        .map_err(|object_id: Vec<_>| {
            anyhow!(
                "GitRef::read: expected object_id to have 20 bytes, got {}",
                object_id.len()
            )
        })?);

        let name = iter.collect::<String>();

        Ok(Self { object_id, name })
    }
}

#[derive(Debug)]
struct GitCapabilities(Vec<String>);

impl GitCapabilities {
    fn read<T: IntoIterator<Item = char>>(iter: T) -> Result<Self> {
        let capabilities = iter
            .into_iter()
            .collect::<String>()
            .split(' ')
            .map(|str| str.to_owned())
            .collect();
        Ok(Self(capabilities))
    }
}

static UPLOAD_PACK_CONTENT_TYPE: &str = "application/x-git-upload-pack-request";
#[derive(Debug)]
struct WantPkt {
    object_id: Sha,
}

impl PktMessage for WantPkt {}

impl ToString for WantPkt {
    fn to_string(&self) -> String {
        format!("want {}", hex::encode(&self.object_id))
    }
}

#[derive(Debug)]
struct HavePkt {
    object_id: Sha,
}

impl PktMessage for HavePkt {}
impl ToString for HavePkt {
    fn to_string(&self) -> String {
        format!("have {}", hex::encode(&self.object_id))
    }
}

trait ToPktLine: Sized {
    fn to_pkt_line(self) -> PktLine;
}

impl ToPktLine for PktLine {
    fn to_pkt_line(self) -> PktLine {
        self
    }
}

impl ToPktLine for Vec<u8> {
    fn to_pkt_line(self) -> PktLine {
        PktLine::BinaryDataPkt(self)
    }
}

impl ToPktLine for String {
    fn to_pkt_line(self) -> PktLine {
        PktLine::StringDataPkt(self)
    }
}

impl<T: PktMessage + ToString> ToPktLine for T {
    fn to_pkt_line(self) -> PktLine {
        PktLine::StringDataPkt(self.to_string())
    }
}

trait PktMessage {}

#[derive(Debug)]
struct Packfile {
    #[allow(dead_code)]
    version: u32,
    #[allow(dead_code)]
    checksum: Sha,
    chunks: Vec<PackfileObject>,
}

impl Packfile {
    fn read<T: IntoIterator<Item = u8>>(iter: T) -> Result<Self> {
        let mut iter = iter.into_iter().peekable();
        assert_eq!(
            iter.by_ref().take(4).collect::<Vec<_>>(),
            b"PACK",
            "Packfile::read: packfiles should start with \"PACK\""
        );

        let version =
            u32::from_be_bytes(read_array(iter.by_ref()).with_context(|| {
                anyhow!("Packfile::read: failed to convert version bytes to u32")
            })?);
        assert_eq!(
            version, 2,
            "Packfile::read: expected version 2, got {version}"
        );

        let object_amount = u32::from_be_bytes(read_array(iter.by_ref()).with_context(|| {
            anyhow!("Packfile::read: failed to convert object amount bytes to u32")
        })?);

        println!("object_amount: {object_amount}");
        let (binary_data, checksum) = {
            let mut rest: Vec<_> = iter.collect();
            let checksum = Sha(rest.split_off(rest.len() - 20).try_into().map_err(|_| {
                anyhow!("Packfile::read: failed to convert checksum bytes to [u8; 20]")
            })?);
            (rest, checksum)
        };
        println!("checksum: {checksum:?}");

        let mut bytes_read = 0;

        let chunks: Vec<_> = (0..object_amount)
            .map(|_| -> Result<_> {
                let (obj, bytes_read_obj) = PackfileObject::decode(&binary_data[bytes_read..])
                    .with_context(|| anyhow!("Packfile::read: failed to decode object"))?;
                bytes_read += usize::try_from(bytes_read_obj).with_context(|| {
                    anyhow!("Packfile::read: failed to convert bytes_read_obj usize")
                })?;
                Ok(obj)
            })
            .collect::<Result<_, _>>()
            .with_context(|| "Packfile::read: failed to read chunks")?;

        Ok(Packfile {
            version,
            checksum,
            chunks,
        })
    }
}

const VARINT_ENCODING_BITS: u8 = 7;
const VARINT_CONTINUE_FLAG: u8 = 1 << VARINT_ENCODING_BITS;
const VARINT_OBJ_TYPE_FLAG: u8 = 0b01110000;
const VARINT_FIRST_BYTE_ENCONDING_BITS: u8 = 4;

fn read_variable_length_integer<T: IntoIterator<Item = u8>>(
    iter: T,
    get_obj_type: bool,
) -> Result<(usize, Option<u8>, u8)> {
    let mut iter = iter.into_iter();
    let mut obj_type = None;
    let mut value: usize = 0;
    let mut length: u8 = 0;
    let mut bytes_read: u8 = 0;

    loop {
        bytes_read += 1;
        let byte = iter
            .next()
            .ok_or_else(|| anyhow!("failed to read variable length integer"))?;
        let is_last = (byte & VARINT_CONTINUE_FLAG) == 0;
        let (data, offset) = if obj_type.is_some() || !get_obj_type {
            (byte & !VARINT_CONTINUE_FLAG, VARINT_ENCODING_BITS)
        } else {
            obj_type = Some((byte & !VARINT_CONTINUE_FLAG) >> VARINT_FIRST_BYTE_ENCONDING_BITS);
            (
                byte & !VARINT_CONTINUE_FLAG & !VARINT_OBJ_TYPE_FLAG,
                VARINT_FIRST_BYTE_ENCONDING_BITS,
            )
        };
        value |= (data as usize) << length;
        if is_last {
            break;
        }
        length += offset;
    }
    Ok((value, obj_type, bytes_read))
}

fn read_array<const N: usize, T: IntoIterator<Item = u8>>(iter: T) -> Result<[u8; N]> {
    iter.into_iter()
        .take(N)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|vec: Vec<_>| {
            anyhow!(
                "failed to read array: expected {N} bytes, got {}",
                vec.len()
            )
        })
}

#[derive(Debug, Clone)]
enum PackfileObject {
    Commit(Commit),
    Tree(Tree),
    Blob(Blob),
    ObjRefDelta(ObjRefDelta),
}

#[derive(Debug, Clone)]
struct ObjRefDelta {
    base_obj_size: usize,
    target_obj_size: usize,
    obj_name: Sha,
    instructions: Vec<DeltaInstruction>,
}

impl PackfileObject {
    fn decode(content: &[u8]) -> Result<(Self, u64)> {
        let (expected_size, obj_type, bytes_read_varint) =
            read_variable_length_integer(content.into_iter().copied(), true)
                .with_context(|| anyhow!("PackfileObject::decode: failed to read object size"))?;
        let obj_type = obj_type.ok_or_else(|| {
          anyhow!("PackfileObject::decode: failed to read variable length integer: couldn't find object type")
      })?;
        let bytes_read_varint = usize::try_from(bytes_read_varint).with_context(|| {
            anyhow!("PackfileObject::decode: failed to convert bytes_read_varint to usize")
        })?;
        let content = &content[bytes_read_varint..];
        let bytes_read_varint = u64::try_from(bytes_read_varint).with_context(|| {
            anyhow!("PackfileObject::decode: failed to convert bytes_read_varint to u64")
        })?;

        let decode_zlib = |content: &[u8]| -> Result<(Vec<u8>, u64)> {
            let (content, bytes_read) = decompress_slice(content)?;
            assert_eq!(
              expected_size,
              content.len(),
              "PackfileObject::decode({obj_type}): object size doesn't match decompressed content size"
          );
            Ok((content, bytes_read))
        };

        match obj_type {
            1 => {
                let (content, bytes_read) = decode_zlib(content)?;
                Ok((
                    Self::Commit(Commit::decode_body(content)?),
                    bytes_read + bytes_read_varint,
                ))
            }
            2 => {
                let (content, bytes_read) = decode_zlib(content)?;
                Ok((
                    Self::Tree(Tree::decode_body(content)?),
                    bytes_read + bytes_read_varint,
                ))
            }
            3 => {
                let (content, bytes_read) = decode_zlib(content)?;
                Ok((
                    Self::Blob(Blob::new(content)),
                    bytes_read + bytes_read_varint,
                ))
            }
            7 => {
                let obj_name = Sha(content.get(..20).ok_or_else(|| {
                  anyhow!(
                      "PackfileObject::decode({obj_type}): expected object name to be 20 bytes, got {}",
                      content.len()
                  )
              })?.to_vec().try_into().map_err(|_| {
                  anyhow!(
                      "PackfileObject::decode({obj_type}): failed to convert object name to Sha"
                  )
              })?);
                let (content, bytes_read) = decode_zlib(&content.get(20..).ok_or_else(|| {
                  anyhow!(
                      "PackfileObject::decode({obj_type}): content bytes are missing, expected more than 20 bytes in content but got {}",
                      content.len()
                  )
              })?)?;
                let mut content = content.into_iter();
                let (base_obj_size, ..) = read_variable_length_integer(content.by_ref(), false)
                    .with_context(|| {
                        anyhow!("PackfileObject::decode: failed to read object size")
                    })?;
                let (target_obj_size, ..) = read_variable_length_integer(content.by_ref(), false)
                    .with_context(|| {
                    anyhow!("PackfileObject::decode: failed to read object size")
                })?;
                let instructions = DeltaInstruction::read_many(content).collect::<Result<Vec<_>>>().with_context(|| {
                  anyhow!("PackfileObject::decode({obj_type}): failed to parse delta instructions")
              })?;
                let obj = Self::ObjRefDelta(ObjRefDelta {
                    base_obj_size,
                    target_obj_size,
                    instructions,
                    obj_name,
                });
                Ok((obj, bytes_read + 20 + bytes_read_varint))
            }
            _ => bail!("PackfileObject::decode({obj_type}): unsupported object type"),
        }
    }
}
#[derive(Debug, Clone)]
enum DeltaInstruction {
    Copy { offset: usize, length: usize },
    Insert(BlobContent),
}

impl DeltaInstruction {
    fn read<T: IntoIterator<Item = u8>>(iter: T) -> Result<Self> {
        let mut iter = iter.into_iter().peekable();
        let first_byte = iter
            .next()
            .ok_or_else(|| anyhow!("DeltaInstruction::read: empty iterator"))?;
        let is_insert = (first_byte & 0b1000_0000) == 0;
        if is_insert {
            let byte_count = first_byte as usize;
            // println!("byte_count: {byte_count} ({first_byte:#08b})");
            Ok(Self::Insert(
                iter.take(byte_count).collect::<Vec<_>>().into(),
            ))
        } else {
            let flags = first_byte & !0b1000_0000;
            // println!("flags: {flags:#07b}");
            let mut offset: usize = 0;
            for i in 0..4 {
                if (flags & (1 << i)) != 0 {
                    let to_apply = (iter
                        .next()
                        .ok_or_else(|| anyhow!("DeltaInstruction::read: expected offset byte"))?
                        as usize)
                        << (i * 8);
                    offset |= to_apply;
                }
            }
            let mut length: usize = 0;
            for i in 4..6 {
                if (flags & (1 << i)) != 0 {
                    let to_apply = (iter
                        .next()
                        .ok_or_else(|| anyhow!("DeltaInstruction::read: expected size byte"))?
                        as usize)
                        << ((i - 4) * 8);
                    length |= to_apply;
                }
            }
            Ok(Self::Copy { offset, length })
        }
    }

    fn read_many<T: IntoIterator<Item = u8>>(iter: T) -> impl Iterator<Item = Result<Self>> {
        let mut iter = iter.into_iter().peekable();
        std::iter::from_fn(move || {
            if iter.peek().is_some() {
                Some(
                    Self::read(&mut iter)
                        .with_context(|| "DeltaInstruction::read_many: failed to read instruction"),
                )
            } else {
                None
            }
        })
    }

    fn apply(instructions: &Vec<DeltaInstruction>, source: &[u8]) -> Vec<u8> {
        let mut output = vec![];
        for instruction in instructions {
            match instruction {
                DeltaInstruction::Copy { offset, length } => {
                    output.extend(&source[*offset..*offset + *length]);
                }
                DeltaInstruction::Insert(data) => {
                    output.extend(data.as_ref());
                }
            }
        }
        output
    }
}
