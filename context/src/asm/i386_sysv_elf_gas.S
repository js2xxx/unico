/* 0x30 */
/*
            Copyright Oliver Kowalke 2009.
   Distributed under the Boost Software License, Version 1.0.
      (See accompanying file LICENSE_1_0.txt or copy at
          http://www.boost.org/LICENSE_1_0.txt)
*/

/****************************************************************************************
 *                                                                                      *
 *  ----------------------------------------------------------------------------------  *
 *  |    0    |    1    |    2    |    3    |    4     |    5    |    6    |    7    |  *
 *  ----------------------------------------------------------------------------------  *
 *  |   0x0   |   0x4   |   0x8   |   0xc   |   0x10   |   0x14  |   0x18  |   0x1c  |  *
 *  ----------------------------------------------------------------------------------  *
 *  | fc_mxcsr|fc_x87_cw|  guard  |   EDI   |   ESI    |   EBX   |   EBP   |   EIP   |  *
 *  ----------------------------------------------------------------------------------  *
 *  ----------------------------------------------------------------------------------  *
 *  |    8    |    9    |    10   |    11   |    12    |    13   |    14   |    15   |  *
 *  ----------------------------------------------------------------------------------  *
 *  |   0x20  |   0x24  |   0x28  |                                                  |  *
 *  ----------------------------------------------------------------------------------  *
 *  |  hidden |    to   |   data  |                                                  |  *
 *  ----------------------------------------------------------------------------------  *
 *                                                                                      *
 ****************************************************************************************/

.file "jump_i386_sysv_elf_gas.S"
.text
.globl jump_fcontext
.align 2
.type jump_fcontext,@function
jump_fcontext:
    leal  -0x1c(%esp), %esp  /* prepare stack */

#if !defined(BOOST_USE_TSX)
    stmxcsr  (%esp)     /* save MMX control- and status-word */
    fnstcw   0x4(%esp)  /* save x87 control-word */
#endif

#if defined(BOOST_CONTEXT_TLS_STACK_PROTECTOR)
    movl  %gs:0x14, %ecx    /* read stack guard from TLS record */
    movl  %ecx, 0x8(%esp)   /* save stack guard */
#endif

    movl  %edi, 0xc(%esp)   /* save EDI */
    movl  %esi, 0x10(%esp)  /* save ESI */
    movl  %ebx, 0x14(%esp)  /* save EBX */
    movl  %ebp, 0x18(%esp)  /* save EBP */

    /* store ESP (pointing to context-data) in ECX */
    movl  %esp, %ecx

    /* first arg of jump_fcontext() == fcontext to jump to */
    movl  0x24(%esp), %eax

    /* second arg of jump_fcontext() == data to be transferred */
    movl  0x28(%esp), %edx

    /* restore ESP (pointing to context-data) from EAX */
    movl  %eax, %esp

    /* address of returned transport_t */
    movl 0x20(%esp), %eax
    /* return parent fcontext_t */
    movl  %ecx, (%eax)
    /* return data */
    movl %edx, 0x4(%eax)

    movl  0x1c(%esp), %ecx  /* restore EIP */

#if !defined(BOOST_USE_TSX)
    ldmxcsr  (%esp)     /* restore MMX control- and status-word */
    fldcw    0x4(%esp)  /* restore x87 control-word */
#endif

#if defined(BOOST_CONTEXT_TLS_STACK_PROTECTOR)
    movl  0x8(%esp), %edx  /* load stack guard */
    movl  %edx, %gs:0x14   /* restore stack guard to TLS record */
#endif

    movl  0xc(%esp), %edi  /* restore EDI */
    movl  0x10(%esp), %esi  /* restore ESI */
    movl  0x14(%esp), %ebx  /* restore EBX */
    movl  0x18(%esp), %ebp  /* restore EBP */

    leal  0x24(%esp), %esp  /* prepare stack */

    /* jump to context */
    jmp *%ecx
.size jump_fcontext,.-jump_fcontext

/* Mark that we don't need executable stack.  */
.section .note.GNU-stack,"",%progbits
/*
            Copyright Oliver Kowalke 2009.
   Distributed under the Boost Software License, Version 1.0.
      (See accompanying file LICENSE_1_0.txt or copy at
          http://www.boost.org/LICENSE_1_0.txt)
*/

/****************************************************************************************
 *                                                                                      *
 *  ----------------------------------------------------------------------------------  *
 *  |    0    |    1    |    2    |    3    |    4     |    5    |    6    |    7    |  *
 *  ----------------------------------------------------------------------------------  *
 *  |   0x0   |   0x4   |   0x8   |   0xc   |   0x10   |   0x14  |   0x18  |   0x1c  |  *
 *  ----------------------------------------------------------------------------------  *
 *  | fc_mxcsr|fc_x87_cw|  guard  |   EDI   |   ESI    |   EBX   |   EBP   |   EIP   |  *
 *  ----------------------------------------------------------------------------------  *
 *  ----------------------------------------------------------------------------------  *
 *  |    8    |    9    |    10   |    11   |    12    |    13   |    14   |    15   |  *
 *  ----------------------------------------------------------------------------------  *
 *  |   0x20  |   0x24  |   0x28  |                                                  |  *
 *  ----------------------------------------------------------------------------------  *
 *  |  hidden |    to   |   data  |                                                  |  *
 *  ----------------------------------------------------------------------------------  *
 *                                                                                      *
 ****************************************************************************************/

.file "make_i386_sysv_elf_gas.S"
.text
.globl make_fcontext
.align 2
.type make_fcontext,@function
make_fcontext:
    /* first arg of make_fcontext() == top of context-stack */
    movl  0x4(%esp), %eax

    /* reserve space for first argument of context-function
       eax might already point to a 16byte border */
    leal  -0x8(%eax), %eax

    /* shift address in EAX to lower 16 byte boundary */
    andl  $-16, %eax

    /* reserve space for context-data on context-stack, and align the stack */
    leal  -0x34(%eax), %eax

    /* third arg of make_fcontext() == address of context-function */
    /* stored in EBX */
    movl  0xc(%esp), %ecx
    movl  %ecx, 0x14(%eax)

    /* save MMX control- and status-word */
    stmxcsr (%eax)
    /* save x87 control-word */
    fnstcw  0x4(%eax)

#if defined(BOOST_CONTEXT_TLS_STACK_PROTECTOR)
    /* save stack guard */
    movl  %gs:0x14, %ecx    /* read stack guard from TLS record */
    movl  %ecx, 0x8(%eax)   /* save stack guard */
#endif

    /* return transport_t */
    /* FCTX == EDI, DATA == ESI */
    leal  0xc(%eax), %ecx
    movl  %ecx, 0x20(%eax)

    /* compute abs address of label trampoline */
    call  1f
    /* address of trampoline 1 */
1:  popl  %ecx
    /* compute abs address of label trampoline */
    addl  $trampoline-1b, %ecx
    /* save address of trampoline as return address */
    /* will be entered after calling jump_fcontext() first time */
    movl  %ecx, 0x1c(%eax)

    /* compute abs address of label finish */
    call  2f
    /* address of label 2 */
2:  popl  %ecx
    /* compute abs address of label finish */
    addl  $finish-2b, %ecx
    /* save address of finish as return-address for context-function */
    /* will be entered after context-function returns */
    movl  %ecx, 0x18(%eax)

    ret /* return pointer to context-data */

trampoline:
    /* move transport_t for entering context-function */
    movl  %edi, (%esp)
    movl  %esi, 0x4(%esp)
    pushl %ebp
    /* jump to context-function */
    jmp *%ebx

finish:
    call  3f
    /* address of label 3 */
3:  popl  %ebx
    /* compute address of GOT and store it in EBX */
    addl  $_GLOBAL_OFFSET_TABLE_+[.-3b], %ebx

    /* exit code is zero */
    xorl  %eax, %eax
    movl  %eax, (%esp)
    /* exit application */
    call  _exit@PLT
    hlt
.size make_fcontext,.-make_fcontext

/* Mark that we don't need executable stack.  */
.section .note.GNU-stack,"",%progbits
/*
            Copyright Oliver Kowalke 2009.
   Distributed under the Boost Software License, Version 1.0.
      (See accompanying file LICENSE_1_0.txt or copy at
          http://www.boost.org/LICENSE_1_0.txt)
*/

/****************************************************************************************
 *                                                                                      *
 *  ----------------------------------------------------------------------------------  *
 *  |    0    |    1    |    2    |    3    |    4     |    5    |    6    |    7    |  *
 *  ----------------------------------------------------------------------------------  *
 *  |   0x0   |   0x4   |   0x8   |   0xc   |   0x10   |   0x14  |   0x18  |   0x1c  |  *
 *  ----------------------------------------------------------------------------------  *
 *  | fc_mxcsr|fc_x87_cw|  guard  |   EDI   |   ESI    |   EBX   |   EBP   |   EIP   |  *
 *  ----------------------------------------------------------------------------------  *
 *  ----------------------------------------------------------------------------------  *
 *  |    8    |    9    |    10   |    11   |    12    |    13   |    14   |    15   |  *
 *  ----------------------------------------------------------------------------------  *
 *  |   0x20  |   0x24  |   0x28  |                                                  |  *
 *  ----------------------------------------------------------------------------------  *
 *  |  hidden |    to   |   data  |                                                  |  *
 *  ----------------------------------------------------------------------------------  *
 *                                                                                      *
 ****************************************************************************************/

.file "ontop_i386_sysv_elf_gas.S"
.text
.globl ontop_fcontext
.align 2
.type ontop_fcontext,@function
ontop_fcontext:
    leal  -0x1c(%esp), %esp  /* prepare stack */

#if !defined(BOOST_USE_TSX)
    stmxcsr  (%esp)     /* save MMX control- and status-word */
    fnstcw   0x4(%esp)  /* save x87 control-word */
#endif

#if defined(BOOST_CONTEXT_TLS_STACK_PROTECTOR)
    movl  %gs:0x14, %ecx    /* read stack guard from TLS record */
    movl  %ecx, 0x8(%esp)   /* save stack guard */
#endif

    movl  %edi, 0xc(%esp)  /* save EDI */
    movl  %esi, 0x10(%esp)  /* save ESI */
    movl  %ebx, 0x14(%esp)  /* save EBX */
    movl  %ebp, 0x18(%esp)  /* save EBP */

    /* store ESP (pointing to context-data) in ECX */
    movl  %esp, %ecx

    /* first arg of ontop_fcontext() == fcontext to jump to */
    movl  0x24(%esp), %eax

    /* pass parent fcontext_t */
    movl  %ecx, 0x24(%eax)

    /* second arg of ontop_fcontext() == data to be transferred */
    movl  0x28(%esp), %ecx

    /* pass data */
    movl %ecx, 0x28(%eax)

    /* third arg of ontop_fcontext() == ontop-function */
    movl  0x2c(%esp), %ecx

    /* restore ESP (pointing to context-data) from EAX */
    movl  %eax, %esp

    /* address of returned transport_t */
    movl 0x20(%esp), %eax
    /* return parent fcontext_t */
    movl  %ecx, (%eax)
    /* return data */
    movl %edx, 0x4(%eax)

#if !defined(BOOST_USE_TSX)
    ldmxcsr  (%esp)     /* restore MMX control- and status-word */
    fldcw    0x4(%esp)  /* restore x87 control-word */
#endif

#if defined(BOOST_CONTEXT_TLS_STACK_PROTECTOR)
    movl  0x8(%esp), %edx  /* load stack guard */
    movl  %edx, %gs:0x14   /* restore stack guard to TLS record */
#endif

    movl  0xc(%esp), %edi  /* restore EDI */
    movl  0x10(%esp), %esi  /* restore ESI */
    movl  0x14(%esp), %ebx  /* restore EBX */
    movl  0x18(%esp), %ebp  /* restore EBP */

    leal  0x1c(%esp), %esp  /* prepare stack */

    /* jump to context */
    jmp *%ecx
.size ontop_fcontext,.-ontop_fcontext

/* Mark that we don't need executable stack.  */
.section .note.GNU-stack,"",%progbits
