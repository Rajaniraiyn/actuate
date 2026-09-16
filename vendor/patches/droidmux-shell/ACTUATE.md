# Vendored shell completion patch

Source: https://github.com/poapoauu/DroidMux at d21e3d729a9d233fd34ce27e5c2244c45c4c9a8e. MIT OR Apache-2.0 licenses retained.

After receiving a complete Shell v2 exit packet, a concurrent remote CLSE can make final local cleanup return StreamClosed or RemoteClosed. Preserve the explicit exit code in precisely that case. Missing exit, malformed framing, session loss and other transport errors remain errors. Legacy EOF does not become a successful explicit exit.

Tests include WRTE containing stdout/stderr/exit immediately followed by remote CLSE, plus focused cleanup cases distinguishing logical closure from session failure.
