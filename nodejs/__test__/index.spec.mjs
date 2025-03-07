import test from 'ava'

import { JsClient } from '../index.js'

test('init test', async (t) => {
  const client = await JsClient.initLocal()
  client.chunkGet();
})
