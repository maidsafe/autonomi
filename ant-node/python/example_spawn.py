from antnode import NetworkSpawner
import asyncio

async def main():
    spawner = NetworkSpawner()
    # Use the new with_bootstrap_config method instead of with_local
    spawner.with_bootstrap_config(local=True)
    network = await spawner.spawn()
    addr = await network.bootstrap_peer()
    print(f"Bootstrap peer: {addr}")

asyncio.run(main())
