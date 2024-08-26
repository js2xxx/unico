/* 0x40 */
/*
            Copyright Oliver Kowalke 2009.
            Copyright Thomas Sailer 2013.
   Distributed under the Boost Software License, Version 1.0.
      (See accompanying file LICENSE_1_0.txt or copy at
            http://www.boost.org/LICENSE_1_0.txt)
*/

/*************************************************************************************
*  --------------------------------------------------------------------------------- *
*  |    0    |    1    |    2    |    3    |    4    |    5    |    6    |    7    | *
*  --------------------------------------------------------------------------------- *
*  |    0h   |   04h   |   08h   |   0ch   |   010h  |   014h  |   018h  |   01ch  | *
*  --------------------------------------------------------------------------------- *
*  | fc_mxcsr|fc_x87_cw| fc_strg |fc_deallo|  limit  |   base  |  fc_seh |   EDI   | *
*  --------------------------------------------------------------------------------- *
*  --------------------------------------------------------------------------------- *
*  |    8    |    9    |   10    |    11   |    12   |    13   |    14   |    15   | *
*  --------------------------------------------------------------------------------- *
*  |   020h  |  024h   |  028h   |   02ch  |   030h  |   034h  |   038h  |   03ch  | *
*  --------------------------------------------------------------------------------- *
*  |   ESI   |   EBX   |   EBP   |   EIP   |    to   |   data  |  EH NXT |SEH HNDLR| *
*  --------------------------------------------------------------------------------- *
**************************************************************************************/

.file	"jump_i386_ms_pe_clang_gas.S"
.text
.p2align 4,,15

/* mark as using no unregistered SEH handlers */
.globl	@feat.00
.def	@feat.00;	.scl	3;	.type	0;	.endef
.set    @feat.00,   1

.globl	_jump_fcontext
.def	_jump_fcontext;	.scl	2;	.type	32;	.endef
_jump_fcontext:
    /* prepare stack */
    leal  -0x2c(%esp), %esp

#if !defined(BOOST_USE_TSX)
    /* save MMX control- and status-word */
    stmxcsr  (%esp)
    /* save x87 control-word */
    fnstcw  0x4(%esp)
#endif

    /* load NT_TIB */
    movl  %fs:(0x18), %edx
    /* load fiber local storage */
    movl  0x10(%edx), %eax
    movl  %eax, 0x8(%esp)
    /* load current dealloction stack */
    movl  0xe0c(%edx), %eax
    movl  %eax, 0xc(%esp)
    /* load current stack limit */
    movl  0x8(%edx), %eax
    movl  %eax, 0x10(%esp)
    /* load current stack base */
    movl  0x4(%edx), %eax
    movl  %eax, 0x14(%esp)
    /* load current SEH exception list */
    movl  (%edx), %eax
    movl  %eax, 0x18(%esp)

    movl  %edi, 0x1c(%esp)  /* save EDI */
    movl  %esi, 0x20(%esp)  /* save ESI */
    movl  %ebx, 0x24(%esp)  /* save EBX */
    movl  %ebp, 0x28(%esp)  /* save EBP */

    /* store ESP (pointing to context-data) in EAX */
    movl  %esp, %eax

    /* firstarg of jump_fcontext() == fcontext to jump to */
    movl  0x30(%esp), %ecx
    
    /* restore ESP (pointing to context-data) from ECX */
    movl  %ecx, %esp

#if !defined(BOOST_USE_TSX)
    /* restore MMX control- and status-word */
    ldmxcsr  (%esp)
    /* restore x87 control-word */
    fldcw  0x4(%esp)
#endif

    /* restore NT_TIB into EDX */
    movl  %fs:(0x18), %edx
    /* restore fiber local storage */
    movl  0x8(%esp), %ecx
    movl  %ecx, 0x10(%edx)
    /* restore current deallocation stack */
    movl  0xc(%esp), %ecx
    movl  %ecx, 0xe0c(%edx)
    /* restore current stack limit */
    movl  0x10(%esp), %ecx
    movl  %ecx, 0x8(%edx)
    /* restore current stack base */
    movl  0x14(%esp), %ecx
    movl  %ecx, 0x4(%edx)
    /* restore current SEH exception list */
    movl  0x18(%esp), %ecx
    movl  %ecx, (%edx)

    movl  0x2c(%esp), %ecx  /* restore EIP */

    movl  0x1c(%esp), %edi  /* restore EDI */
    movl  0x20(%esp), %esi  /* restore ESI */
    movl  0x24(%esp), %ebx  /* restore EBX */
    movl  0x28(%esp), %ebp  /* restore EBP */

    /* prepare stack */
    leal  0x30(%esp), %esp

    /* return transfer_t */
    /* FCTX == EAX, DATA == EDX */
    movl  0x34(%eax), %edx

    /* jump to context */
    jmp *%ecx

.section .drectve
.ascii " -export:\"_jump_fcontext\""
/*
            Copyright Oliver Kowalke 2009.
            Copyright Thomas Sailer 2013.
   Distributed under the Boost Software License, Version 1.0.
      (See accompanying file LICENSE_1_0.txt or copy at
            http://www.boost.org/LICENSE_1_0.txt)
*/

/*************************************************************************************
*  --------------------------------------------------------------------------------- *
*  |    0    |    1    |    2    |    3    |    4    |    5    |    6    |    7    | *
*  --------------------------------------------------------------------------------- *
*  |    0h   |   04h   |   08h   |   0ch   |   010h  |   014h  |   018h  |   01ch  | *
*  --------------------------------------------------------------------------------- *
*  | fc_mxcsr|fc_x87_cw| fc_strg |fc_deallo|  limit  |   base  |  fc_seh |   EDI   | *
*  --------------------------------------------------------------------------------- *
*  --------------------------------------------------------------------------------- *
*  |    8    |    9    |   10    |    11   |    12   |    13   |    14   |    15   | *
*  --------------------------------------------------------------------------------- *
*  |   020h  |  024h   |  028h   |   02ch  |   030h  |   034h  |   038h  |   03ch  | *
*  --------------------------------------------------------------------------------- *
*  |   ESI   |   EBX   |   EBP   |   EIP   |    to   |   data  |  EH NXT |SEH HNDLR| *
*  --------------------------------------------------------------------------------- *
**************************************************************************************/

.file	"make_i386_ms_pe_clang_gas.S"
.text
.p2align 4,,15

/* mark as using no unregistered SEH handlers */
.globl	@feat.00
.def	@feat.00;	.scl	3;	.type	0;	.endef
.set    @feat.00,   1

.globl	_make_fcontext
.def	_make_fcontext;	.scl	2;	.type	32;	.endef
_make_fcontext:
    /* first arg of make_fcontext() == top of context-stack */
    movl  0x04(%esp), %eax

    /* reserve space for first argument of context-function */
    /* EAX might already point to a 16byte border */
    leal  -0x8(%eax), %eax

    /* shift address in EAX to lower 16 byte boundary */
    andl  $-16, %eax

    /* reserve space for context-data on context-stack */
    /* size for fc_mxcsr .. EIP + return-address for context-function */
    /* on context-function entry: (ESP -0x4) % 8 == 0 */
    /* additional space is required for SEH */
    leal  -0x40(%eax), %eax

    /* save MMX control- and status-word */
    stmxcsr  (%eax)
    /* save x87 control-word */
    fnstcw  0x4(%eax)

    /* first arg of make_fcontext() == top of context-stack */
    movl  0x4(%esp), %ecx
    /* save top address of context stack as 'base' */
    movl  %ecx, 0x14(%eax)
    /* second arg of make_fcontext() == size of context-stack */
    movl  0x8(%esp), %edx
    /* negate stack size for LEA instruction (== substraction) */
    negl  %edx
    /* compute bottom address of context stack (limit) */
    leal  (%ecx,%edx), %ecx
    /* save bottom address of context-stack as 'limit' */
    movl  %ecx, 0x10(%eax)
    /* save bottom address of context-stack as 'dealloction stack' */
    movl  %ecx, 0xc(%eax)
	/* set fiber-storage to zero */
	xorl  %ecx, %ecx
    movl  %ecx, 0x8(%eax)

    /* third arg of make_fcontext() == address of context-function */
    /* stored in EBX */
    movl  0xc(%esp), %ecx
    movl  %ecx, 0x24(%eax)

    /* compute abs address of label trampoline */
    movl  $trampoline, %ecx
    /* save address of trampoline as return-address for context-function */
    /* will be entered after calling jump_fcontext() first time */
    movl  %ecx, 0x2c(%eax)

    /* compute abs address of label finish */
    movl  $finish, %ecx
    /* save address of finish as return-address for context-function */
    /* will be entered after context-function returns */
    movl  %ecx, 0x28(%eax)

    /* traverse current seh chain to get the last exception handler installed by Windows */
    /* note that on Windows Server 2008 and 2008 R2, SEHOP is activated by default */
    /* the exception handler chain is tested for the presence of ntdll.dll!FinalExceptionHandler */
    /* at its end by RaiseException all seh andlers are disregarded if not present and the */
    /* program is aborted */
    /* load NT_TIB into ECX */
    movl  %fs:(0x0), %ecx

walk:
    /* load 'next' member of current SEH into EDX */
    movl  (%ecx), %edx
    /* test if 'next' of current SEH is last (== 0xffffffff) */
    incl  %edx
    jz  found
    decl  %edx
    /* exchange content; ECX contains address of next SEH */
    xchgl  %ecx, %edx
    /* inspect next SEH */
    jmp  walk

found:
    /* load 'handler' member of SEH == address of last SEH handler installed by Windows */
    movl  0x04(%ecx), %ecx
    /* save address in ECX as SEH handler for context */
    movl  %ecx, 0x3c(%eax)
    /* set ECX to -1 */
    movl  $0xffffffff, %ecx
    /* save ECX as next SEH item */
    movl  %ecx, 0x38(%eax)
    /* load address of next SEH item */
    leal  0x38(%eax), %ecx
    /* save next SEH */
    movl  %ecx, 0x18(%eax)

    /* return pointer to context-data */
    ret

trampoline:
    /* move transport_t for entering context-function */
    /* FCTX == EAX, DATA == EDX */
    movl  %eax, (%esp)
    movl  %edx, 0x4(%esp)
    /* label finish as return-address */
    pushl %ebp
    /* jump to context-function */
    jmp  *%ebx

finish:
    /* ESP points to same address as ESP on entry of context function + 0x4 */
    xorl  %eax, %eax
    /* exit code is zero */
    movl  %eax, (%esp)
    /* exit application */
    call  __exit
    hlt

.def	__exit;	.scl	2;	.type	32;	.endef  /* standard C library function */

.section .drectve
.ascii " -export:\"_make_fcontext\""
/*
            Copyright Oliver Kowalke 2009.
            Copyright Thomas Sailer 2013.
   Distributed under the Boost Software License, Version 1.0.
      (See accompanying file LICENSE_1_0.txt or copy at
            http://www.boost.org/LICENSE_1_0.txt)
*/

/*************************************************************************************
*  --------------------------------------------------------------------------------- *
*  |    0    |    1    |    2    |    3    |    4    |    5    |    6    |    7    | *
*  --------------------------------------------------------------------------------- *
*  |    0h   |   04h   |   08h   |   0ch   |   010h  |   014h  |   018h  |   01ch  | *
*  --------------------------------------------------------------------------------- *
*  | fc_mxcsr|fc_x87_cw| fc_strg |fc_deallo|  limit  |   base  |  fc_seh |   EDI   | *
*  --------------------------------------------------------------------------------- *
*  --------------------------------------------------------------------------------- *
*  |    8    |    9    |   10    |    11   |    12   |    13   |    14   |    15   | *
*  --------------------------------------------------------------------------------- *
*  |   020h  |  024h   |  028h   |   02ch  |   030h  |   034h  |   038h  |   03ch  | *
*  --------------------------------------------------------------------------------- *
*  |   ESI   |   EBX   |   EBP   |   EIP   |    to   |   data  |  EH NXT |SEH HNDLR| *
*  --------------------------------------------------------------------------------- *
**************************************************************************************/

.file	"ontop_i386_ms_pe_clang_gas.S"
.text
.p2align 4,,15

/* mark as using no unregistered SEH handlers */
.globl	@feat.00
.def	@feat.00;	.scl	3;	.type	0;	.endef
.set    @feat.00,   1

.globl	_ontop_fcontext
.def	_ontop_fcontext;	.scl	2;	.type	32;	.endef
_ontop_fcontext:
    /* prepare stack */
    leal  -0x2c(%esp), %esp

#if !defined(BOOST_USE_TSX)
    /* save MMX control- and status-word */
    stmxcsr  (%esp)
    /* save x87 control-word */
    fnstcw  0x4(%esp)
#endif

    /* load NT_TIB */
    movl  %fs:(0x18), %edx
    /* load fiber local storage */
    movl  0x10(%edx), %eax
    movl  %eax, 0x8(%esp)
    /* load current dealloction stack */
    movl  0xe0c(%edx), %eax
    movl  %eax, 0xc(%esp)
    /* load current stack limit */
    movl  0x8(%edx), %eax
    movl  %eax, 0x10(%esp)
    /* load current stack base */
    movl  0x4(%edx), %eax
    movl  %eax, 0x14(%esp)
    /* load current SEH exception list */
    movl  (%edx), %eax
    movl  %eax, 0x18(%esp)

    movl  %edi, 0x1c(%esp)  /* save EDI */
    movl  %esi, 0x20(%esp)  /* save ESI */
    movl  %ebx, 0x24(%esp)  /* save EBX */
    movl  %ebp, 0x28(%esp)  /* save EBP */

    /* store ESP (pointing to context-data) in ECX */
    movl  %esp, %ecx

    /* first arg of ontop_fcontext() == fcontext to jump to */
    movl  0x30(%esp), %eax

	/* pass parent fcontext_t */
	movl  %ecx, 0x30(%eax)

    /* second arg of ontop_fcontext() == data to be transferred */
    movl  0x34(%esp), %ecx

	/* pass data */
	movl  %ecx, 0x34(%eax)

    /* third arg of ontop_fcontext() == ontop-function */
    movl  0x38(%esp), %ecx

    /* restore ESP (pointing to context-data) from EDX */
    movl  %eax, %esp

#if !defined(BOOST_USE_TSX)
    /* restore MMX control- and status-word */
    ldmxcsr  (%esp)
    /* restore x87 control-word */
    fldcw  0x4(%esp)
#endif

    /* restore NT_TIB into EDX */
    movl  %fs:(0x18), %edx
    /* restore fiber local storage */
    movl  0x8(%esp), %eax
    movl  %eax, 0x10(%edx)
    /* restore current deallocation stack */
    movl  0xc(%esp), %eax
    movl  %eax, 0xe0c(%edx)
    /* restore current stack limit */
    movl  0x10(%esp), %eax
    movl  %eax, 0x08(%edx)
    /* restore current stack base */
    movl  0x14(%esp), %eax
    movl  %eax, 0x04(%edx)
    /* restore current SEH exception list */
    movl  0x18(%esp), %eax
    movl  %eax, (%edx)

    movl  0x1c(%esp), %edi  /* restore EDI */
    movl  0x20(%esp), %esi  /* restore ESI */
    movl  0x24(%esp), %ebx  /* restore EBX */
    movl  0x28(%esp), %ebp  /* restore EBP */

    /* prepare stack */
    leal  0x2c(%esp), %esp

    /* keep return-address on stack */

    /* jump to context */
    jmp  *%ecx

.section .drectve
.ascii " -export:\"_ontop_fcontext\""
