# What's this?

Analysis of frequency of instructions (1-2 parameterized opcodes) in the Lama VM bytecode.

## Usage

```
./target/release/lamanyzer -l <path_to_bytecode_file>
```

## Memory usage

As reported by `valgrind --tool=massif ./target/release/lamanyzer -l ../Lama/Sort.bc`:
```
KB
10.34^                                                             :        :@
 |                                                     #:::::::::@:::@:@@@
 |                                                     #:::::::::@:::@:@@@
 |                                                     #:::::::::@:::@:@@@
 |                                                  :: #:::::::::@:::@:@@@
 |                                        :         :  #:::::::::@:::@:@@@
 |                                        :         :  #:::::::::@:::@:@@@
 |                                        ::::::::::: :#:::::::::@:::@:@@@
 |                                        ::        : :#:::::::::@:::@:@@@
 |                              ::::::::::::        : :#:::::::::@:::@:@@@
 |                     :      : :         ::        : :#:::::::::@:::@:@@@
 |                     :      :::         ::        : :#:::::::::@:::@:@@@
 |                     :    @@: :         ::        : :#:::::::::@:::@:@@@
 |                     :   :@ : :         ::        : :#:::::::::@:::@:@@@
 |                     : :::@ : :         ::        : :#:::::::::@:::@:@@@
 |                     : : :@ : :         ::        : :#:::::::::@:::@:@@@
 |                     : : :@ : :         ::        : :#:::::::::@:::@:@@@
 |               :::::@::: :@ : :         ::        : :#:::::::::@:::@:@@@
 |               :    @::: :@ : :         ::        : :#:::::::::@:::@:@@@
 |               :    @::: :@ : :         ::        : :#:::::::::@:::@:@@@
0 +----------------------------------------------------------------------->Mi
 0                                                                   1.278
```

10 Kb is less then the required 16x constraint:
```
~/Uni/VirtualMachines/lama-rs  =>  stat ../Lama/Sort.bc
  File: ../Lama/Sort.bc
  Size: 794             Blocks: 8          IO Block: 4096   regular file
Device: 10304h/66308d   Inode: 15217056    Links: 1
Access: (0664/-rw-rw-r--)  Uid: ( 1000/safonoff)   Gid: ( 1000/safonoff)
Access: 2026-02-24 20:33:13.268463565 +0300
Modify: 2026-02-19 23:30:35.794773506 +0300
Change: 2026-02-19 23:30:35.794773506 +0300
 Birth: 2026-02-17 19:07:03.499437351 +0300
```

Which gives us: 16 × 794B = 12.4KB

## Example

Example of frequency analysis of `Sort.bc`:
<details>
<summary>Output</summary>

```
=> ./target/release/lamanyzer -l ../Lama/Sort.bc
DROP: 31
DUP: 28
ELEM: 21
CONST 1: 16
CONST 1; ELEM: 13
DROP; DUP: 11
DUP; CONST 1: 11
CONST 0: 11
DROP; DROP: 10
CONST 0; ELEM: 8
LOAD function argument 0: 7
DUP; CONST 0: 7
ELEM; DROP: 7
END: 5
DUP; DUP: 4
SEXP 0 2: 4
ARRAY 2: 3
LOAD local variable 0: 3
LOAD local variable 3: 3
STORE local variable 0: 3
ELEM; STORE local variable 0: 3
DUP; ARRAY 2: 3
STORE local variable 0; DROP: 3
CALL 351 1: 3
JMP 762: 3
CALL Barray 2: 3
DUP; TAG 0 2: 2
ELEM; CONST 0: 2
BINOP EQ: 2
LOAD local variable 1: 2
ELEM; CONST 1: 2
TAG 0 2: 2
CALL 43 1: 2
BEGIN 1 0: 2
JMP 350: 2
JMP 116: 2
SEXP 0 2; CALL Barray 2: 2
CALL 151 1: 2
JMP 386: 1
CJMP 428 ISNONZERO: 1
DROP; LOAD local variable 5: 1
STORE local variable 4: 1
CJMP 197 ISNONZERO: 1
BEGIN 1 1: 1
ELEM; DUP: 1
BINOP SUB; CALL 43 1: 1
DROP; LINE 15: 1
LINE 14; LOAD function argument 0: 1
BINOP EQ; CJMP 191 ISZERO: 1
LINE 5; LOAD local variable 3: 1
LINE 7; LOAD local variable 2: 1
ELEM; STORE local variable 1: 1
CONST 10000: 1
CJMP 274 ISZERO: 1
CJMP 637 ISNONZERO: 1
BINOP EQ; CJMP 274 ISZERO: 1
LINE 24; LOAD function argument 0: 1
LINE 18: 1
TAG 0 2; CJMP 392 ISNONZERO: 1
DROP; JMP 386: 1
LOAD function argument 0; CALL 351 1: 1
LINE 7: 1
TAG 0 2; CJMP 428 ISNONZERO: 1
LOAD local variable 0; JMP 350: 1
ARRAY 2; CJMP 280 ISNONZERO: 1
BEGIN 2 0: 1
CONST 0; JMP 116: 1
BEGIN 1 6; LINE 3: 1
LOAD function argument 0; CJMP 106 ISZERO: 1
LINE 3: 1
DROP; JMP 734: 1
DROP; JMP 336: 1
DROP; LINE 16: 1
STORE local variable 2; DROP: 1
CJMP 428 ISNONZERO; DROP: 1
LOAD local variable 3; LOAD local variable 0: 1
LOAD local variable 2; CALL 351 1: 1
CONST 0; LINE 9: 1
STORE local variable 1; DROP: 1
LOAD local variable 4: 1
JMP 262: 1
LINE 20: 1
CJMP 600 ISZERO: 1
CJMP 392 ISNONZERO: 1
LINE 27: 1
LINE 16; LOAD local variable 0: 1
CALL 117 1: 1
LINE 20; LOAD function argument 0: 1
LINE 14: 1
BEGIN 1 0; LINE 24: 1
LOAD local variable 3; LOAD local variable 4: 1
CJMP 106 ISZERO; LOAD function argument 0: 1
JMP 734: 1
LOAD function argument 0; DUP: 1
LINE 18; LINE 20: 1
CJMP 280 ISNONZERO: 1
LINE 16: 1
LOAD function argument 0; CONST 1: 1
LINE 25; LINE 27: 1
STORE local variable 5; DROP: 1
FAIL 14 9: 1
STORE local variable 1: 1
BINOP GT; CJMP 600 ISZERO: 1
LINE 24: 1
LOAD local variable 2: 1
LINE 5: 1
CONST 1; BINOP EQ: 1
LINE 6: 1
LINE 9: 1
STORE local variable 2: 1
LOAD local variable 5; LOAD local variable 3: 1
LINE 6; LOAD local variable 1: 1
STORE local variable 4; DROP: 1
STORE local variable 3: 1
ARRAY 2; CJMP 197 ISNONZERO: 1
CONST 0; BINOP EQ: 1
LOAD local variable 3; LOAD local variable 1: 1
CJMP 274 ISZERO; DUP: 1
ELEM; STORE local variable 3: 1
DUP; DROP: 1
CONST 10000; CALL 43 1: 1
CJMP 191 ISZERO: 1
BINOP SUB: 1
BINOP GT: 1
LOAD local variable 0; SEXP 0 2: 1
LOAD local variable 1; LOAD local variable 3: 1
ARRAY 2; CJMP 637 ISNONZERO: 1
LOAD local variable 5: 1
LINE 27; CONST 10000: 1
DROP; LINE 5: 1
BEGIN 1 1; LINE 14: 1
ELEM; SEXP 0 2: 1
DROP; JMP 715: 1
LOAD function argument 0; CALL Barray 2: 1
LOAD function argument 0; CALL 151 1: 1
LINE 3; LOAD function argument 0: 1
LOAD local variable 1; BINOP GT: 1
LINE 15; LOAD local variable 0: 1
STORE local variable 3; DROP: 1
CJMP 600 ISZERO; CONST 1: 1
LINE 15: 1
LOAD function argument 0; LOAD function argument 0: 1
BEGIN 1 0; LINE 18: 1
JMP 336: 1
JMP 715: 1
CJMP 106 ISZERO: 1
CONST 1; BINOP SUB: 1
DROP; JMP 262: 1
SEXP 0 2; JMP 116: 1
LOAD local variable 0; CALL 151 1: 1
BEGIN 1 6: 1
CONST 1; LINE 6: 1
SEXP 0 2; CALL 351 1: 1
CJMP 637 ISNONZERO; DROP: 1
ELEM; STORE local variable 5: 1
STORE local variable 5: 1
ELEM; STORE local variable 4: 1
LOAD local variable 4; SEXP 0 2: 1
CJMP 191 ISZERO; DUP: 1
FAIL 7 17: 1
LINE 9; LOAD function argument 0: 1
DROP; CONST 0: 1
LINE 25: 1
ELEM; STORE local variable 2: 1
BEGIN 2 0; LINE 25: 1
```
</details>
