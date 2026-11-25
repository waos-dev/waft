# waft-video-codecs

What a video stream says of itself, read the same way for every system's decoder: a codec's setup bytes and the units of its chunks, what the stream says of its pictures (how far it reorders them, their colour, their pixels' shape), and the order pictures are shown in where a system's decoder leaves that to its caller.

It takes `waft-embedder`'s video decoder types and provides, per codec, what a host's decoder asks of a stream before handing it to the system. The hosts' decoders use it (`waft-embedder-apple`'s VideoToolbox, and the Android, Windows and Linux decoders as they move onto it), so a codec's rules are written once and a system's decoder only opens the system's session. H.264 so far.
