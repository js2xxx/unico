/* 0x80 */
/*
            Copyright Oliver Kowalke 2009.
   Distributed under the Boost Software License, Version 1.0.
      (See accompanying file LICENSE_1_0.txt or copy at
          http://www.boost.org/LICENSE_1_0.txt)
*/

/*******************************************************
 *                                                     *
 *  -------------------------------------------------  *
 *  |  0  |  1  |  2  |  3  |  4  |  5  |  6  |  7  |  *
 *  -------------------------------------------------  *
 *  | 0x0 | 0x4 | 0x8 | 0xc | 0x10| 0x14| 0x18| 0x1c|  *
 *  -------------------------------------------------  *
 *  | s16 | s17 | s18 | s19 | s20 | s21 | s22 | s23 |  *
 *  -------------------------------------------------  *
 *  -------------------------------------------------  *
 *  |  8  |  9  |  10 |  11 |  12 |  13 |  14 |  15 |  *
 *  -------------------------------------------------  *
 *  | 0x20| 0x24| 0x28| 0x2c| 0x30| 0x34| 0x38| 0x3c|  *
 *  -------------------------------------------------  *
 *  | s24 | s25 | s26 | s27 | s28 | s29 | s30 | s31 |  *
 *  -------------------------------------------------  *
 *  -------------------------------------------------  *
 *  |  16 |  17 |  18 |  19 |  20 |  21 |  22 |  23 |  *
 *  -------------------------------------------------  *
 *  | 0x40| 0x44| 0x48| 0x4c| 0x50| 0x54| 0x58| 0x5c|  *
 *  -------------------------------------------------  *
 *  |hiddn|  v1 |  v2 |  v3 |  v4 |  v5 |  v6 |  v7 |  *
 *  -------------------------------------------------  *
 *  -------------------------------------------------  *
 *  |  24 |  25 |  26 |  27 |  28 |  29 |  30 |  31 |  *
 *  -------------------------------------------------  *
 *  | 0x60| 0x64| 0x68| 0x6c| 0x70| 0x74| 0x78| 0x7c|  *
 *  -------------------------------------------------  *
 *  |  v8 |  lr |  pc | FCTX| DATA|                 |  *
 *  -------------------------------------------------  *
 *                                                     *
 *******************************************************/

.file "jump_arm_aapcs_elf_gas.S"
.text
.globl jump_fcontext
.align 2
.type jump_fcontext,%function
.syntax unified
jump_fcontext:
    @ save LR as PC
    push {lr}
    @ save hidden,V1-V8,LR
    push {a1,v1-v8,lr}

    @ prepare stack for FPU
    sub  sp, sp, #64
#if (defined(__VFP_FP__) && !defined(__SOFTFP__))
    @ save S16-S31
    vstmia sp, {d8-d15}
#endif

    @ store RSP (pointing to context-data) in A1
    mov  a1, sp

    @ restore RSP (pointing to context-data) from A2
    mov  sp, a2

#if (defined(__VFP_FP__) && !defined(__SOFTFP__))
    @ restore S16-S31
    vldmia  sp, {d8-d15}
#endif
    @ prepare stack for FPU
    add  sp, sp, #64

    @ restore hidden,V1-V8,LR
    pop {a4,v1-v8,lr}

    @ return transfer_t from jump
    str  a1, [a4, #0]
    str  a3, [a4, #4]
    @ pass transfer_t as first arg in context function
    @ A1 == FCTX, A2 == DATA
    mov  a2, a3

    @ restore PC
    pop {pc}
.size jump_fcontext,.-jump_fcontext

@ Mark that we don't need executable stack.
.section .note.GNU-stack,"",%progbits
/*
            Copyright Oliver Kowalke 2009.
   Distributed under the Boost Software License, Version 1.0.
      (See accompanying file LICENSE_1_0.txt or copy at
          http://www.boost.org/LICENSE_1_0.txt)
*/

/*******************************************************
 *                                                     *
 *  -------------------------------------------------  *
 *  |  0  |  1  |  2  |  3  |  4  |  5  |  6  |  7  |  *
 *  -------------------------------------------------  *
 *  | 0x0 | 0x4 | 0x8 | 0xc | 0x10| 0x14| 0x18| 0x1c|  *
 *  -------------------------------------------------  *
 *  | s16 | s17 | s18 | s19 | s20 | s21 | s22 | s23 |  *
 *  -------------------------------------------------  *
 *  -------------------------------------------------  *
 *  |  8  |  9  |  10 |  11 |  12 |  13 |  14 |  15 |  *
 *  -------------------------------------------------  *
 *  | 0x20| 0x24| 0x28| 0x2c| 0x30| 0x34| 0x38| 0x3c|  *
 *  -------------------------------------------------  *
 *  | s24 | s25 | s26 | s27 | s28 | s29 | s30 | s31 |  *
 *  -------------------------------------------------  *
 *  -------------------------------------------------  *
 *  |  16 |  17 |  18 |  19 |  20 |  21 |  22 |  23 |  *
 *  -------------------------------------------------  *
 *  | 0x40| 0x44| 0x48| 0x4c| 0x50| 0x54| 0x58| 0x5c|  *
 *  -------------------------------------------------  *
 *  |hiddn|  v1 |  v2 |  v3 |  v4 |  v5 |  v6 |  v7 |  *
 *  -------------------------------------------------  *
 *  -------------------------------------------------  *
 *  |  24 |  25 |  26 |  27 |  28 |  29 |  30 |  31 |  *
 *  -------------------------------------------------  *
 *  | 0x60| 0x64| 0x68| 0x6c| 0x70| 0x74| 0x78| 0x7c|  *
 *  -------------------------------------------------  *
 *  |  v8 |  lr |  pc | FCTX| DATA|                 |  *
 *  -------------------------------------------------  *
 *                                                     *
 *******************************************************/

.file "make_arm_aapcs_elf_gas.S"
.text
.globl make_fcontext
.align 2
.type make_fcontext,%function
.syntax unified
make_fcontext:
    @ shift address in A1 to lower 16 byte boundary
    bic  a1, a1, #15

    @ reserve space for context-data on context-stack
    sub  a1, a1, #124

    @ third arg of make_fcontext() == address of context-function
    str  a3, [a1, #104]

    @ compute address of returned transfer_t
    add  a2, a1, #108
    mov  a3, a2
    str  a3, [a1, #64]

    @ compute abs address of label finish
    adr  a2, finish
    @ save address of finish as return-address for context-function
    @ will be entered after context-function returns
    str  a2, [a1, #100]

#if (defined(__VFP_FP__) && !defined(__SOFTFP__))
#endif

    bx  lr @ return pointer to context-data

finish:
    @ exit code is zero
    mov  a1, #0
    @ exit application
    bl  _exit@PLT
.size make_fcontext,.-make_fcontext

@ Mark that we don't need executable stack.
.section .note.GNU-stack,"",%progbits
/*
            Copyright Oliver Kowalke 2009.
   Distributed under the Boost Software License, Version 1.0.
      (See accompanying file LICENSE_1_0.txt or copy at
          http://www.boost.org/LICENSE_1_0.txt)
*/

/*******************************************************
 *                                                     *
 *  -------------------------------------------------  *
 *  |  0  |  1  |  2  |  3  |  4  |  5  |  6  |  7  |  *
 *  -------------------------------------------------  *
 *  | 0x0 | 0x4 | 0x8 | 0xc | 0x10| 0x14| 0x18| 0x1c|  *
 *  -------------------------------------------------  *
 *  | s16 | s17 | s18 | s19 | s20 | s21 | s22 | s23 |  *
 *  -------------------------------------------------  *
 *  -------------------------------------------------  *
 *  |  8  |  9  |  10 |  11 |  12 |  13 |  14 |  15 |  *
 *  -------------------------------------------------  *
 *  | 0x20| 0x24| 0x28| 0x2c| 0x30| 0x34| 0x38| 0x3c|  *
 *  -------------------------------------------------  *
 *  | s24 | s25 | s26 | s27 | s28 | s29 | s30 | s31 |  *
 *  -------------------------------------------------  *
 *  -------------------------------------------------  *
 *  |  16 |  17 |  18 |  19 |  20 |  21 |  22 |  23 |  *
 *  -------------------------------------------------  *
 *  | 0x40| 0x44| 0x48| 0x4c| 0x50| 0x54| 0x58| 0x5c|  *
 *  -------------------------------------------------  *
 *  |hiddn|  v1 |  v2 |  v3 |  v4 |  v5 |  v6 |  v7 |  *
 *  -------------------------------------------------  *
 *  -------------------------------------------------  *
 *  |  24 |  25 |  26 |  27 |  28 |  29 |  30 |  31 |  *
 *  -------------------------------------------------  *
 *  | 0x60| 0x64| 0x68| 0x6c| 0x70| 0x74| 0x78| 0x7c|  *
 *  -------------------------------------------------  *
 *  |  v8 |  lr |  pc | FCTX| DATA|                 |  *
 *  -------------------------------------------------  *
 *                                                     *
 *******************************************************/

.file "ontop_arm_aapcs_elf_gas.S"
.text
.globl ontop_fcontext
.align 2
.type ontop_fcontext,%function
.syntax unified
ontop_fcontext:
    @ save LR as PC
    push {lr}
    @ save hidden,V1-V8,LR
    push {a1,v1-v8,lr}

    @ prepare stack for FPU
    sub  sp, sp, #64
#if (defined(__VFP_FP__) && !defined(__SOFTFP__))
    @ save S16-S31
    vstmia sp, {d8-d15}
#endif

    @ store RSP (pointing to context-data) in A1
    mov  a1, sp

    @ restore RSP (pointing to context-data) from A2
    mov  sp, a2

    @ store parent context in A2
    mov  a2, a1

#if (defined(__VFP_FP__) && !defined(__SOFTFP__))
    @ restore S16-S31
    vldmia  sp, {d8-d15}
#endif
    @ prepare stack for FPU
    add  sp, sp, #64

    @ restore hidden,V1-V8,LR
    pop {a1,v1-v8,lr}

    @ return transfer_t from jump
    str  a2, [a1, #0]
    str  a3, [a1, #4]
    @ pass transfer_t as first arg in context function
    @ A1 == hidden, A2 == FCTX, A3 == DATA

    @ skip PC
    add  sp, sp, #4

    @ jump to ontop-function
    bx  a4
.size ontop_fcontext,.-ontop_fcontext

@ Mark that we don't need executable stack.
.section .note.GNU-stack,"",%progbits
