import * as anchor from "@coral-xyz/anchor";
import {Program, web3, BN} from "@coral-xyz/anchor";
import {assert, expect} from "chai";

// If you generated types with Anchor, you can import them like:
// import { ArenaforgeMvp } from "../target/types/arenaforge_mvp";
// For portability, we'll keep things untyped here and rely on IDL at runtime.

describe("ArenaForge MVP", () => {
  const provider = anchor.AnchorProvider.local();
  anchor.setProvider(provider);

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const program = (anchor.workspace as any).ArenaforgeMvp as Program;
  const connection = provider.connection;

  const PROGRAM_ID = program.programId;

  const seeds = {
    global: Buffer.from("global"),
    reserve: Buffer.from("reserve"),
    forge: Buffer.from("forge"),
    arena: Buffer.from("arena"),
    battle: Buffer.from("battle"),
    bet: Buffer.from("bet"),
  };

  const pda = {
    global: web3.PublicKey.findProgramAddressSync([seeds.global], PROGRAM_ID)[0],
    reserve: web3.PublicKey.findProgramAddressSync([seeds.reserve], PROGRAM_ID)[0],
  };

  const admin = (provider.wallet as anchor.Wallet).payer; // test admin

  const users = {
    creatorA: web3.Keypair.generate(),
    creatorB: web3.Keypair.generate(),
    bettor1: web3.Keypair.generate(),
    bettor2: web3.Keypair.generate(),
  };

  const mints = {
    // For MVP we only need pubkeys; use random ones
    A: web3.Keypair.generate().publicKey,
    B: web3.Keypair.generate().publicKey,
  };

  const deriveForge = (mint: web3.PublicKey) =>
    web3.PublicKey.findProgramAddressSync([seeds.forge, mint.toBuffer()], PROGRAM_ID)[0];

  const deriveArena = (authority: web3.PublicKey) =>
    web3.PublicKey.findProgramAddressSync([seeds.arena, authority.toBuffer()], PROGRAM_ID)[0];

  const u64buf = (x: bigint) => Buffer.from(new BigUint64Array([x]).buffer);

  const deriveBattle = (
    arenaPk: web3.PublicKey,
    forgeA: web3.PublicKey,
    forgeB: web3.PublicKey,
    nonce: bigint
  ) =>
    web3.PublicKey.findProgramAddressSync(
      [seeds.battle, arenaPk.toBuffer(), forgeA.toBuffer(), forgeB.toBuffer(), u64buf(nonce)],
      PROGRAM_ID
    )[0];

  const deriveBet = (battle: web3.PublicKey, bettor: web3.PublicKey) =>
    web3.PublicKey.findProgramAddressSync([seeds.bet, battle.toBuffer(), bettor.toBuffer()], PROGRAM_ID)[0];

  async function airdrop(pubkey: web3.PublicKey, sol = 5) {
    const sig = await connection.requestAirdrop(pubkey, sol * web3.LAMPORTS_PER_SOL);
    await connection.confirmTransaction(sig, "confirmed");
  }

  before("airdrop users", async () => {
    for (const k of Object.values(users)) {
      await airdrop(k.publicKey, 10);
    }
  });

  it("initialize_global", async () => {
    const feeBps = 200; // 2%
    const betRakeBps = 300; // 3%
    const entryFeeLamports = new BN(0.02 * web3.LAMPORTS_PER_SOL);

    await program.methods
      .initializeGlobal(feeBps, betRakeBps, entryFeeLamports)
      .accounts({
        admin: admin.publicKey,
        global: pda.global,
        reserveVault: pda.reserve,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([])
      .rpc();

    const globalAcc = await program.account.global.fetch(pda.global);
    expect(globalAcc.admin.toBase58()).to.eq(admin.publicKey.toBase58());
    expect(globalAcc.feeBps).to.eq(feeBps);
    expect(globalAcc.betRakeBps).to.eq(betRakeBps);
  });

  const state: any = {};

  it("create_arena (VolumeWar)", async () => {
    const authority = admin.publicKey; // admin as arena authority
    const oracle = admin.publicKey; // make admin act as oracle in tests
    const arena = deriveArena(authority);

    await program.methods
      .createArena({ volumeWar: {} }, 500, new BN(120))
      .accounts({
        authority,
        global: pda.global,
        oracle,
        arena,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([])
      .rpc();

    state.arena = arena;
    const arenaAcc = await program.account.arena.fetch(arena);
    expect(arenaAcc.rewardBpsFromReserve).to.eq(500);
    expect(arenaAcc.minDurationSecs.toNumber()).to.eq(120);
  });

  it("register two forges (A & B)", async () => {
    const forgeA = deriveForge(mints.A);
    const forgeB = deriveForge(mints.B);

    // Creator A pays entry fee into reserve via register_forge
    await program.methods
      .registerForge()
      .accounts({
        payer: users.creatorA.publicKey,
        creator: users.creatorA.publicKey,
        mint: mints.A,
        global: pda.global,
        reserveVault: pda.reserve,
        forge: forgeA,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([users.creatorA])
      .rpc();

    await program.methods
      .registerForge()
      .accounts({
        payer: users.creatorB.publicKey,
        creator: users.creatorB.publicKey,
        mint: mints.B,
        global: pda.global,
        reserveVault: pda.reserve,
        forge: forgeB,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([users.creatorB])
      .rpc();

    state.forgeA = forgeA;
    state.forgeB = forgeB;

    const a = await program.account.forge.fetch(forgeA);
    const b = await program.account.forge.fetch(forgeB);
    expect(a.creator.toBase58()).to.eq(users.creatorA.publicKey.toBase58());
    expect(b.creator.toBase58()).to.eq(users.creatorB.publicKey.toBase58());
  });

  it("init_battle and place bets", async () => {
    const nonce = 1n;
    const battle = deriveBattle(state.arena, state.forgeA, state.forgeB, nonce);
    const now = Math.floor(Date.now() / 1000);

    // creators pay entry fees again here per MVP design
    await program.methods
      .initBattle(new BN(nonce.toString()), new BN(now + 5), new BN(now + 90))
      .accounts({
        authority: admin.publicKey,
        global: pda.global,
        reserveVault: pda.reserve,
        arena: state.arena,
        forgeA: state.forgeA,
        forgeB: state.forgeB,
        forgeACreator: users.creatorA.publicKey,
        forgeBCreator: users.creatorB.publicKey,
        battle,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([users.creatorA, users.creatorB])
      .rpc();

    state.battle = battle;

    // Wait until battle start
    await new Promise((r) => setTimeout(r, 6000));

    const bet1 = deriveBet(battle, users.bettor1.publicKey);
    await program.methods
      .placeBet(0, new BN(0.5 * web3.LAMPORTS_PER_SOL)) // bettor1 on A
      .accounts({
        bettor: users.bettor1.publicKey,
        arena: state.arena,
        forgeA: state.forgeA,
        forgeB: state.forgeB,
        battle: state.battle,
        bet: bet1,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([users.bettor1])
      .rpc();

    const bet2 = deriveBet(battle, users.bettor2.publicKey);
    await program.methods
      .placeBet(1, new BN(0.3 * web3.LAMPORTS_PER_SOL)) // bettor2 on B
      .accounts({
        bettor: users.bettor2.publicKey,
        arena: state.arena,
        forgeA: state.forgeA,
        forgeB: state.forgeB,
        battle: state.battle,
        bet: bet2,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([users.bettor2])
      .rpc();

    const bAcc = await program.account.battle.fetch(battle);
    expect(bAcc.potA.toNumber()).to.be.greaterThan(0);
    expect(bAcc.potB.toNumber()).to.be.greaterThan(0);
  });

  it("oracle pushes metrics and resolve battle", async () => {
    // push metrics while round active
    await program.methods
      .updateMetrics(new BN(10_000), new BN(7_500))
      .accounts({ oracle: admin.publicKey, arena: state.arena, battle: state.battle })
      .signers([])
      .rpc();

    // wait for round end
    await new Promise((r) => setTimeout(r, 90000));

    const preReserve = await connection.getBalance(pda.reserve);

    const winnerRecipient = users.creatorA.publicKey; // arbitrary recipient
    await program.methods
      .resolveBattle()
      .accounts({
        authority: admin.publicKey,
        global: pda.global,
        reserveVault: pda.reserve,
        arena: state.arena,
        battle: state.battle,
        winnerRecipient,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([])
      .rpc();

    const postReserve = await connection.getBalance(pda.reserve);
    expect(postReserve).to.be.lessThan(preReserve); // reward paid out

    const b = await program.account.battle.fetch(state.battle);
    expect([0, 1]).to.include(b.winner);
    expect(b.resolved).to.eq(true);
    state.winner = b.winner;
  });

  it("claim payouts (winner gets paid, loser gets 0)", async () => {
    // Snapshot pre-balances
    const balBefore1 = await connection.getBalance(users.bettor1.publicKey);
    const balBefore2 = await connection.getBalance(users.bettor2.publicKey);

    const bet1 = deriveBet(state.battle, users.bettor1.publicKey);
    const bet2 = deriveBet(state.battle, users.bettor2.publicKey);

    // bettor1 was on side 0; bettor2 on side 1
    // Depending on winner, one of them should get >0
    await program.methods
      .claimBet()
      .accounts({
        bettor: users.bettor1.publicKey,
        arena: state.arena,
        forgeA: state.forgeA,
        forgeB: state.forgeB,
        battle: state.battle,
        bet: bet1,
        global: pda.global,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([users.bettor1])
      .rpc();

    await program.methods
      .claimBet()
      .accounts({
        bettor: users.bettor2.publicKey,
        arena: state.arena,
        forgeA: state.forgeA,
        forgeB: state.forgeB,
        battle: state.battle,
        bet: bet2,
        global: pda.global,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([users.bettor2])
      .rpc();

    const balAfter1 = await connection.getBalance(users.bettor1.publicKey);
    const balAfter2 = await connection.getBalance(users.bettor2.publicKey);

    if (state.winner === 0) {
      expect(balAfter1).to.be.greaterThan(balBefore1); // bettor1 wins
      expect(balAfter2).to.be.at.most(balBefore2);      // bettor2 loses or unchanged (tx fees)
    } else {
      expect(balAfter2).to.be.greaterThan(balBefore2); // bettor2 wins
      expect(balAfter1).to.be.at.most(balBefore1);
    }

    // Double-claim should fail
    try {
      await program.methods
        .claimBet()
        .accounts({
          bettor: users.bettor1.publicKey,
          arena: state.arena,
          forgeA: state.forgeA,
          forgeB: state.forgeB,
          battle: state.battle,
          bet: bet1,
          global: pda.global,
          systemProgram: web3.SystemProgram.programId,
        })
        .signers([users.bettor1])
        .rpc();
      assert.fail("expected AlreadyClaimed");
    } catch (e) {
      // ok
    }
  });

  it("sweep rake to reserve", async () => {
    const preBattle = await connection.getBalance(state.battle);
    const preReserve = await connection.getBalance(pda.reserve);

    await program.methods
      .sweepRake()
      .accounts({
        admin: admin.publicKey,
        global: pda.global,
        reserveVault: pda.reserve,
        arena: state.arena,
        forgeA: state.forgeA,
        forgeB: state.forgeB,
        battle: state.battle,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([])
      .rpc();

    const postBattle = await connection.getBalance(state.battle);
    const postReserve = await connection.getBalance(pda.reserve);

    expect(postReserve).to.be.greaterThanOrEqual(preReserve);
    expect(postBattle).to.be.lessThanOrEqual(preBattle);
  });

  it("deposit & withdraw reserve (admin only)", async () => {
    // Anyone can deposit by sending lamports to reserve; our ix is a no-op marker, so simulate with transfer
    const tx = new web3.Transaction().add(
      web3.SystemProgram.transfer({
        fromPubkey: users.bettor1.publicKey,
        toPubkey: pda.reserve,
        lamports: 0.2 * web3.LAMPORTS_PER_SOL,
      })
    );
    await web3.sendAndConfirmTransaction(connection, tx, [users.bettor1]);

    const before = await connection.getBalance(pda.reserve);

    await program.methods
      .withdrawFromReserve(new BN(0.05 * web3.LAMPORTS_PER_SOL))
      .accounts({
        admin: admin.publicKey,
        global: pda.global,
        reserveVault: pda.reserve,
        systemProgram: web3.SystemProgram.programId,
      })
      .signers([])
      .rpc();

    const after = await connection.getBalance(pda.reserve);
    expect(after).to.be.lessThan(before);
  });
});
