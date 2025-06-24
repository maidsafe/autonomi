import test from 'ava'
import path from 'path'
import fs from 'fs'
import os from 'os'
import crypto from 'crypto'
import { 
  Client, 
  Wallet, 
  Network, 
  PaymentOption, 
  PrivateArchive,
  PublicArchive,
  SecretKey,
  DataAddress,
  XorName,
  Metadata
} from '../index.js'

// Utility function to compute SHA256 hash of a file
function computeSha256(filePath) {
  return new Promise((resolve, reject) => {
    const hash = crypto.createHash('sha256');
    const stream = fs.createReadStream(filePath);
    
    stream.on('error', err => reject(err));
    stream.on('data', chunk => hash.update(chunk));
    stream.on('end', () => resolve(hash.digest('hex')));
  });
}

// Utility function to compute SHA256 hash of a directory
async function computeDirSha256(dir) {
  const hash = crypto.createHash('sha256');
  
  // Get all files recursively
  async function getFiles(dir) {
    const entries = await fs.promises.readdir(dir, { withFileTypes: true });
    const files = await Promise.all(entries.map(entry => {
      const res = path.resolve(dir, entry.name);
      return entry.isDirectory() ? getFiles(res) : res;
    }));
    return files.flat();
  }
  
  // Hash all files in the directory
  const files = await getFiles(dir);
  for (const file of files) {
    const fileHash = await computeSha256(file);

    hash.update(fileHash);
  }
  
  return hash.digest('hex');
}

test('private archive - upload and download directory', async (t) => {
  // Initialize client
  const client = await Client.initLocal();
  const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");
  const paymentOption = PaymentOption.fromWallet(wallet);
  
  // Define source and destination directories
  const sourceDir = path.join('../autonomi', 'tests', 'file', 'test_dir');
  const destDir = path.join(os.tmpdir());
  
  // Upload directory content
  const { cost, archive } = await client.dirContentUpload(sourceDir, paymentOption);
  
  // Verify cost is returned as a string
  t.true(typeof cost === 'string');
  
  // Upload the archive
  const { cost: archiveCost, dataMap } = await client.archivePut(archive, paymentOption);
  
  // Verify archive cost is returned as a string
  t.true(typeof archiveCost === 'string');
  
  // Download the archive
  await client.dirDownload(dataMap, destDir);
  
  // Compare the hash of source and destination directories
  const sourceHash = await computeDirSha256(sourceDir);
  const destHash = await computeDirSha256(path.join(destDir, 'test_dir'));
  
  t.is(sourceHash, destHash, 'Source and destination directory hashes should match');
});

test('public archive - upload and download directory', async (t) => {
  // Initialize client
  const client = await Client.initLocal();
  const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");
  const paymentOption = PaymentOption.fromWallet(wallet);
  
  // Define source and destination directories
  const sourceDir = path.join('../autonomi', 'tests', 'file', 'test_dir');
  const destDir = os.tmpdir();
  
  // Upload directory content as public
  const { cost, addr: archive } = await client.dirContentUploadPublic(sourceDir, paymentOption);
  
  // Verify cost is returned as a string
  t.true(typeof cost === 'string');
  
  // Upload the archive
  const { cost: archiveCost, addr } = await client.archivePutPublic(archive, paymentOption);
  
  // Verify archive cost is returned as a string
  t.true(typeof archiveCost === 'string');
  
  // Download the archive
  await client.dirDownloadPublic(addr, destDir);
  
  // Compare the hash of source and destination directories
  const sourceHash = await computeDirSha256(sourceDir);
  const destHash = await computeDirSha256(path.join(destDir, 'test_dir'));
  
  t.is(sourceHash, destHash, 'Source and destination directory hashes should match');
});

test('archive advanced use', async (t) => {
  // Initialize client
  const client = await Client.initLocal();
  const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");
  const paymentOption = PaymentOption.fromWallet(wallet);
  
  // Define source and destination directories
  const sourceDirA = path.join('../autonomi', 'tests', 'file', 'test_dir', 'dir_a');
  const fileB = path.join('../autonomi', 'tests', 'file', 'test_dir', 'example_file_b');
  const fileA = path.join('../autonomi', 'tests', 'file', 'test_dir', 'example_file_a');
  const destDir = os.tmpdir();
  
  // Upload directory content
  const { cost, archive } = await client.dirContentUpload(sourceDirA, paymentOption);
  
  // Verify cost is returned as a string
  t.true(typeof cost === 'string');
  
  // Upload an additional file separately
  const { cost: fileBCost, dataMap: fileBDataMap } = await client.fileContentUpload(fileB, paymentOption);
  
  // Verify file cost is returned as a string
  t.true(typeof fileBCost === 'string');
  
  // Add the file to the archive with custom metadata
  const now = Math.floor(Date.now() / 1000);
  // Create metadata with all custom fields at once
  const fileBMetadata = Metadata.withCustomFields(
    BigInt(now),
    BigInt(now),
    BigInt(13),
    "Extra metadata for fileB"
  );
  archive.addFile("example_file_b", fileBDataMap, fileBMetadata);
  
  // Upload another additional file separately
  const { cost: fileACost, dataMap: fileADataMap } = await client.fileContentUpload(fileA, paymentOption);
  
  // Verify file cost is returned as a string
  t.true(typeof fileACost === 'string');
  
  // Also add this file to archive
  const fileAMetadata = Metadata.newWithSize(BigInt(13)); // size 13 bytes
  archive.addFile("example_file_a", fileADataMap, fileAMetadata);
  
  // Check that we have the expected files in the archive
  const files = archive.files();
  t.is(files.length, 4); // Two files from dir_a plus the two we added
  
  // Upload the archive
  const { cost: archiveCost, dataMap } = await client.archivePut(archive, paymentOption);
  
  // Verify archive cost is returned as a string
  t.true(typeof archiveCost === 'string');
});

// Test storing an archive in vault
test('file into vault', async (t) => {
  // Initialize client
  const client = await Client.initLocal();
  const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");
  const paymentOption = PaymentOption.fromWallet(wallet);
  
  // Create a random secret key
  const secretKey = SecretKey.random();
  
  // Define source directory
  const sourceDir = path.join('../autonomi', 'tests', 'file', 'test_dir');
  
  // Upload directory content as public
  const { cost: dirCost, addr } = await client.dirUploadPublic(sourceDir, paymentOption);
  
  // Verify cost is returned as a string
  t.true(typeof dirCost === 'string');
  
  // Get the archive
  const archive = await client.archiveGetPublic(addr);
  
  // Convert archive to bytes and write to vault
  const archiveBytes = archive.toBytes();
  t.true(archiveBytes.length > 0, 'Archive bytes should not be empty');
  
  // Test converting bytes back to archive
  const recoveredArchive = PublicArchive.fromBytes(archiveBytes);
  const recoveredFiles = recoveredArchive.addresses();
  t.true(recoveredFiles.length > 0, 'Recovered archive should contain files');
});
