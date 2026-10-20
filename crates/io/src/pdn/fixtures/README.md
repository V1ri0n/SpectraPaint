Object-graph (MS-NRBF) sections of files written by Paint.NET 4.21, cut from the test files of
https://github.com/addisonElliott/pypdn (MIT licence, tests/data): everything between the `00 01`
marker after the XML header and the first layer's pixel data. Used by `write_tests.rs` to check the
writer reproduces Paint.NET's bytes.
