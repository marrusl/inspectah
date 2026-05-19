/**
 * Browser-side SHA-512 crypt(3) implementation.
 *
 * Produces hashes in the format: $6$rounds=5000$<salt>$<hash>
 * Compatible with glibc crypt(3) / openssl passwd -6.
 *
 * Reference: Ulrich Drepper's SHA-512 crypt specification.
 * https://www.akkadia.org/drepper/SHA-crypt.txt
 */

const SALT_CHARS =
  "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789./";
const HASH64_CHARS =
  "./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/** Generate a random salt string of the given length. */
function generateSalt(length: number): string {
  const arr = new Uint8Array(length);
  crypto.getRandomValues(arr);
  return Array.from(arr, (b) => SALT_CHARS[b % SALT_CHARS.length]).join("");
}

type Bytes = Uint8Array<ArrayBuffer>;

/** SHA-512 hash via SubtleCrypto. */
async function sha512(data: Bytes): Promise<Bytes> {
  const buf = await crypto.subtle.digest("SHA-512", data);
  return new Uint8Array(buf) as Bytes;
}

/** Concatenate multiple byte arrays. */
function concat(...arrays: Bytes[]): Bytes {
  let total = 0;
  for (const a of arrays) total += a.length;
  const result = new Uint8Array(total) as Bytes;
  let offset = 0;
  for (const a of arrays) {
    result.set(a, offset);
    offset += a.length;
  }
  return result;
}

/** Encode 3 bytes into 4 base64-crypt characters (little-endian order). */
function encode64Triplet(a: number, b: number, c: number, n: number): string {
  let v = (a << 16) | (b << 8) | c;
  let out = "";
  for (let i = 0; i < n; i++) {
    out += HASH64_CHARS[v & 0x3f];
    v >>= 6;
  }
  return out;
}

/** Encode the 64-byte SHA-512 digest into the crypt(3) base64 string. */
function encodeHash(hash: Bytes): string {
  // The permutation order is specified by Drepper's spec.
  return (
    encode64Triplet(hash[0], hash[21], hash[42], 4) +
    encode64Triplet(hash[22], hash[43], hash[1], 4) +
    encode64Triplet(hash[44], hash[2], hash[23], 4) +
    encode64Triplet(hash[3], hash[24], hash[45], 4) +
    encode64Triplet(hash[25], hash[46], hash[4], 4) +
    encode64Triplet(hash[47], hash[5], hash[26], 4) +
    encode64Triplet(hash[6], hash[27], hash[48], 4) +
    encode64Triplet(hash[28], hash[49], hash[7], 4) +
    encode64Triplet(hash[50], hash[8], hash[29], 4) +
    encode64Triplet(hash[9], hash[30], hash[51], 4) +
    encode64Triplet(hash[31], hash[52], hash[10], 4) +
    encode64Triplet(hash[53], hash[11], hash[32], 4) +
    encode64Triplet(hash[12], hash[33], hash[54], 4) +
    encode64Triplet(hash[34], hash[55], hash[13], 4) +
    encode64Triplet(hash[56], hash[14], hash[35], 4) +
    encode64Triplet(hash[15], hash[36], hash[57], 4) +
    encode64Triplet(hash[37], hash[58], hash[16], 4) +
    encode64Triplet(hash[59], hash[17], hash[38], 4) +
    encode64Triplet(hash[18], hash[39], hash[60], 4) +
    encode64Triplet(hash[40], hash[61], hash[19], 4) +
    encode64Triplet(hash[62], hash[20], hash[41], 4) +
    encode64Triplet(0, 0, hash[63], 2)
  );
}

/**
 * Compute a SHA-512 crypt(3) hash.
 *
 * @param password - The plaintext password
 * @param rounds - Number of rounds (default 5000, the glibc default)
 * @returns Hash string in format $6$rounds=5000$<salt>$<hash>
 */
export async function sha512Crypt(
  password: string,
  rounds = 5000,
): Promise<string> {
  const salt = generateSalt(16);
  const encoder = new TextEncoder();
  const keyBytes = encoder.encode(password);
  const saltBytes = encoder.encode(salt);

  // Step 1-3: Compute digest B
  const digestB = await sha512(concat(keyBytes, saltBytes, keyBytes));

  // Step 4-8: Compute digest A
  let digestAInput = concat(keyBytes, saltBytes);

  // Step 9-10: Add bytes from B based on password length
  let remaining = keyBytes.length;
  while (remaining > 64) {
    digestAInput = concat(digestAInput, digestB);
    remaining -= 64;
  }
  digestAInput = concat(digestAInput, digestB.slice(0, remaining));

  // Step 11: Process password length bits
  let len = keyBytes.length;
  while (len > 0) {
    if (len & 1) {
      digestAInput = concat(digestAInput, digestB);
    } else {
      digestAInput = concat(digestAInput, keyBytes);
    }
    len >>= 1;
  }

  // Step 12: Compute digest A
  let digestA = await sha512(digestAInput);

  // Step 13-15: Compute digest DP (key-derived)
  let dpInput = new Uint8Array(0) as Bytes;
  for (let i = 0; i < keyBytes.length; i++) {
    dpInput = concat(dpInput, keyBytes);
  }
  const digestDP = await sha512(dpInput);

  // Step 16: Produce P string
  const p = new Uint8Array(keyBytes.length) as Bytes;
  let pOff = 0;
  while (pOff + 64 <= keyBytes.length) {
    p.set(digestDP, pOff);
    pOff += 64;
  }
  if (pOff < keyBytes.length) {
    p.set(digestDP.slice(0, keyBytes.length - pOff), pOff);
  }

  // Step 17-19: Compute digest DS (salt-derived)
  let dsInput = new Uint8Array(0) as Bytes;
  for (let i = 0; i < 16 + digestA[0]; i++) {
    dsInput = concat(dsInput, saltBytes);
  }
  const digestDS = await sha512(dsInput);

  // Step 20: Produce S string
  const s = new Uint8Array(saltBytes.length) as Bytes;
  let sOff = 0;
  while (sOff + 64 <= saltBytes.length) {
    s.set(digestDS, sOff);
    sOff += 64;
  }
  if (sOff < saltBytes.length) {
    s.set(digestDS.slice(0, saltBytes.length - sOff), sOff);
  }

  // Step 21: Rounds
  let digestC = digestA;
  for (let i = 0; i < rounds; i++) {
    let cInput = new Uint8Array(0) as Bytes;
    if (i & 1) {
      cInput = concat(cInput, p);
    } else {
      cInput = concat(cInput, digestC);
    }
    if (i % 3 !== 0) {
      cInput = concat(cInput, s);
    }
    if (i % 7 !== 0) {
      cInput = concat(cInput, p);
    }
    if (i & 1) {
      cInput = concat(cInput, digestC);
    } else {
      cInput = concat(cInput, p);
    }
    digestC = await sha512(cInput);
  }

  // Step 22: Encode and format
  const encoded = encodeHash(digestC);
  return `$6$rounds=${rounds}$${salt}$${encoded}`;
}
