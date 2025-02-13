from antnode import NetworkSpawner
import asyncio

async def main():
    spawner = NetworkSpawner()
    spawner.with_local(True)
    network = await spawner.spawn()
    addr = await network.bootstrap_peer()
    print(f"Bootstrap peer: {addr}")

asyncio.run(main())
