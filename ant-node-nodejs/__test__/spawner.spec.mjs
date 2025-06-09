import test from 'ava'
import { NetworkSpawner } from '../index.js'

test('construct NetworkSpawner', async (t) => {
    const spawner = new NetworkSpawner({ local: true, size: 20 });
    const runningNetwork = await spawner.spawn();

    const runningNodes = await runningNetwork.runningNodes();
    t.is(runningNodes.length, 20);

    await runningNetwork.shutdown();
});
