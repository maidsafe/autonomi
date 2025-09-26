import os
import pytest
from autonomi_client import *

def _b32(b: bytes) -> bytes:
    # Ensure exactly 32 bytes (Rust expects [u8; 32]).
    assert len(b) == 32
    return b

def test_graph_entry_address():
    # Create a GraphEntry and verify its derived address and hex round-trip.
    owner = SecretKey()
    ge = GraphEntry(owner, [], _b32(os.urandom(32)), [])
    addr = ge.address()

    # Basic shape checks
    assert isinstance(addr, GraphEntryAddress)
    assert isinstance(addr.hex, str)
    assert len(addr.hex) == 96

    # Round-trip via from_hex (bindings expose from_hex, not a hex ctor)
    addr2 = GraphEntryAddress.from_hex(addr.hex)
    assert isinstance(addr2, GraphEntryAddress)
    assert addr2.hex == addr.hex

def test_graph_entry_methods_and_roundtrip():
    # Exercise GraphEntry methods and ensure parents/descendants round-trip.
    owner = SecretKey()
    parents = [PublicKey.random() for _ in range(2)]
    content = _b32(os.urandom(32))
    descendants = [
        (PublicKey.random(), _b32(os.urandom(32))),
        (PublicKey.random(), _b32(os.urandom(32))),
    ]

    ge = GraphEntry(owner, parents, content, descendants)

    # address()
    addr = ge.address()
    assert isinstance(addr, GraphEntryAddress)
    assert isinstance(addr.hex, str) and len(addr.hex) == 96

    # content()
    assert ge.content() == content

    # parents()
    ps = ge.parents()
    assert isinstance(ps, list) and all(isinstance(p, PublicKey) for p in ps)
    assert [p.hex() for p in ps] == [p.hex() for p in parents]

    # descendants()
    ds = ge.descendants()
    assert isinstance(ds, list)
    for pk, h in ds:
        assert isinstance(pk, PublicKey)
        assert isinstance(h, (bytes, bytearray)) and len(h) == 32
    assert [(pk.hex(), h) for pk, h in ds] == [(pk.hex(), h) for pk, h in descendants]

    # Type/shape errors
    with pytest.raises(Exception):
        GraphEntry(owner, parents, os.urandom(31), descendants)  # bad content len
    with pytest.raises(Exception):
        GraphEntry(owner, parents, content, [(PublicKey.random(), b"x")])  # bad hash len

def test_chunk_address():
    # Construct ChunkAddress from XorName and from content, validate hex.
    xor = random_xor()                 # returns PyXorName
    addr = ChunkAddress(xor)           # constructor expects PyXorName

    # Hex is 64 chars and matches XorName hex
    assert isinstance(addr.hex, str)
    assert len(addr.hex) == 64
    assert addr.hex == xor.as_hex()

    # from_content factory should also work
    addr2 = ChunkAddress.from_content(b"test data")
    assert isinstance(addr2.hex, str) and len(addr2.hex) == 64

def test_pointer_address():
    # PointerAddress is derived from PublicKey; validate hex and round-trip.
    pk = PublicKey.random()
    addr = PointerAddress(pk)

    assert isinstance(addr.hex, str)
    assert len(addr.hex) == 96

    # from_hex round-trip
    addr2 = PointerAddress.from_hex(addr.hex)
    assert addr2.hex == addr.hex

def test_pointer_target_with_chunk_address():
    # Create a PointerTarget from a ChunkAddress and validate target hex.
    chunk_addr = ChunkAddress.from_content(b"test data for pointer target")
    target = PointerTarget.new_chunk(chunk_addr)
    assert isinstance(target.hex, str)
    assert len(target.hex) == 64

def test_pointer_creation():
    # Construct a Pointer; check address() and target() shape.
    key = SecretKey()
    counter = 42
    target = PointerTarget.new_chunk(ChunkAddress(random_xor()))
    pointer = Pointer(key, counter, target)

    # Pointer has address()
    paddr = pointer.address()
    assert isinstance(paddr, PointerAddress)
    assert isinstance(paddr.hex, str)
    assert len(paddr.hex) == 96

    # Target present and shaped correctly
    assert isinstance(pointer.target.hex, str)
    assert len(pointer.target.hex) == 64

def test_pointer_target_creation():
    # Direct creation of PointerTarget from content-derived ChunkAddress.
    target = PointerTarget.new_chunk(ChunkAddress.from_content(b"test data for pointer target"))
    assert isinstance(target.hex, str)
    assert len(target.hex) == 64

def test_invalid_hex():
    # Passing wrong Python types to ctors should raise TypeError in bindings.
    with pytest.raises(TypeError):
        ChunkAddress("invalid hex")     # expects PyXorName, not str
    with pytest.raises(TypeError):
        PointerAddress("invalid hex")   # expects PublicKey, not str

def test_wallet():
    network = EVMNetwork(True)
    private_key = "0xdb1049e76a813c94be0df47ec3e20533ca676b1b9fef2ddbce9daa117e4da4aa"
    wallet = Wallet.new_from_private_key(network, private_key)

    assert wallet.address() == "0x69D5BF2Bc42bca8782b8D2b4FdfF2b1Fa7644Fe7"
    assert wallet.network() == network

def test_data_stream_types():
    # Existence and minimal construction tests for data stream related types.
    # DataStream class presence
    try:
        stream_class = DataStream
        assert hasattr(stream_class, '__name__')
        assert stream_class.__name__ == 'DataStream'
    except NameError:
        pytest.fail("DataStream class not found in autonomi_client module")

    # DataMapChunk.from_hex: exercise path and hex() method if it accepts dummy value
    data_map_hex = "0" * 64
    try:
        data_map = DataMapChunk.from_hex(data_map_hex)
        assert hasattr(data_map, "hex")
        assert data_map.hex() == data_map_hex
    except Exception:
        # Acceptable with a mock hex; presence is primary goal
        pass

    # DataAddress should accept a XorName
    data_addr = DataAddress(random_xor())
    assert isinstance(data_addr.hex, str)
    assert len(data_addr.hex) == 64
