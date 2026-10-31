#!/usr/bin/env python3
"""The data behind the systems windows (interface/systems.catalog.json).

Usage: systems_catalog.py <static.db4> <export_win32> <DSO_Godot> <assets>

Achievements (with their categories, icons and the titles they unlock), events
(their texts, hints, advertised items, banner, buttons and the entry their
teleport leads to), the daily challenges, the Wanted Thief's missions, the
collector's album (its books and stickers), the workbench (its tabs, category
tree, recipe books and crafting recipes), the shops' category trees and
banners, the equipment skins, the socket prices and the currency bag's
currencies -- each with its English
text resolved from the client's language files, so the client needs no key
logic. Item icons are the item catalogue's (interface/inventory.items.json).

The windows' runtime artwork (the texture a widget gets from data: event tabs
and banners, daily cards, Wanted cards, workbench and shop tab icons,
achievement and recipe icons) is converted to <assets>/textures/ like the
layouts' atlases (tools/export_ui.py), when missing.
"""
import json
import os
import re
import sqlite3
import sys


def plain(value):
    value = re.sub(r"<br\s*/?>", "\n", value or "", flags=re.I)
    return re.sub(r"<[^>]*>", "", value).strip()


def split(value):
    return [v.strip() for v in str(value or "").replace(";", ",").split(",") if v.strip()]


def export_systems(database, root, godot, assets):
    sys.path.insert(0, godot)
    import export_ui as gx  # DSO_Godot's exporter: language/<lang>/*.xml
    en = gx.load_loca(root, ["en"]).get("en", {})
    text = lambda key, fallback="": plain(en.get(key) or fallback)
    db = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    db.row_factory = sqlite3.Row
    q = lambda sql: db.execute(sql).fetchall()
    textures = set()

    def texture(ref):
        """A runtime texture reference, kept for conversion: 'ui/x' (no extension)."""
        ref = (ref or "").strip().strip("/").replace("\\", "/")
        if ref:
            textures.add(ref)
        return ref

    achievements = {}
    for r in q("SELECT Id, Category, UnlocksTitle, IsHidden, Amount, Icon, Points, EventId FROM _Template_Achievements WHERE EnabledLive=1"):
        achievements[r["Id"]] = dict(
            title=text(f"db.achievements.table_{r['Id']}Title", r["Id"]),
            description=text(f"db.achievements.table_{r['Id']}Desc"),
            category=r["Category"] or "", hidden=bool(r["IsHidden"]), amount=int(r["Amount"] or 1),
            points=int(r["Points"] or 0), event=r["EventId"] or "", icon=texture(r["Icon"]),
            title_text=text(f"db.achievementtitles.table_{r['Id']}Title") if r["UnlocksTitle"] else "",
        )
    categories = [dict(id=r["Category"], parent=r["Parent"] or "", priority=int(r["Priority"] or 0), icon=texture(r["Icon"]),
                       title=text(f"db.achievementscategory.table_{r['Category']}Title", r["Category"]))
                  for r in q("SELECT Category, Parent, Priority, Icon FROM _Template_AchievementsCategory ORDER BY Priority, rowid")]

    # Events: what the event window shows of each (UI::EventProgressWidget):
    # its title and text, the hints (bullet points with their artwork), the
    # items it advertises, its banner, the buttons it offers
    # (AdditionalEventElements), its progress bar's reward thresholds, and its
    # Category, which decides whether the window lists it at all.
    bars = {r["Id"]: r for r in q("SELECT Id, EventRewardItems, EventRewardCurrency, EventCollectibles FROM _Template_EventProgressBar")}
    events = {}
    for r in q("SELECT * FROM _Template_Events"):
        keys = r.keys()
        get = lambda name: r[name] if name in keys else None
        hints = split(get("BulletPointLocaIds"))
        artworks = split(get("BulletPointArtworks"))
        # The bar's rewards: "[page/]threshold:item:amount" (the page the bar
        # shows them on, 0 when unnamed), the Andermant ones
        # "[page/]threshold:amount", and the collectible its progress counts.
        rewards, currency, collectible = [], [], ""
        for bar in split(get("EventProgressBars"))[:1]:
            row = bars.get(bar)
            for entry in (row["EventRewardItems"] or "").split(";") if row else []:
                m = re.match(r"(?:(\d+)/)?(\d+):(.+):(\d+)$", entry.strip())
                if m:
                    # Per-class alternatives (warrior|item,Ranger|item...): the first.
                    item = m.group(3).split(",")[0].split("|")[-1]
                    rewards.append(dict(page=int(m.group(1) or 0), at=int(m.group(2)), item=item, amount=int(m.group(4))))
            for entry in (row["EventRewardCurrency"] or "").split(";") if row else []:
                m = re.match(r"(?:(\d+)/)?(\d+):(\d+)$", entry.strip())
                if m:
                    currency.append(dict(page=int(m.group(1) or 0), at=int(m.group(2)), item="", amount=int(m.group(3))))
            collectibles = [c.split("/")[-1] for c in (row["EventCollectibles"] or "").split(";") if c.strip()] if row else []
            collectible = collectibles[0] if collectibles else ""
        events[r["Id"]] = dict(
            title=text(f"db.event.table_{r['Id']}Title", text(f"db.event.table_{r['Id']}ShortTitle", get("Name") or r["Id"])),
            category=get("Category") or "",
            description=text(f"db.event.table_{r['Id']}Progress")[:600], teleport=get("TeleportTarget") or "", icon=texture(get("IconBrush")),
            hints=[dict(text=text(f"gui.window.event_{h}", h), icon=artworks[n] if n < len(artworks) else "") for n, h in enumerate(hints)],
            advertised=split(get("AdvertisedItems"))[:12], label=texture(get("EventLabel")), banner=texture(get("BannerIcon")),
            elements=split(get("AdditionalEventElements")), rewards=rewards[:80], currency=currency[:80], collectible=collectible,
            offers=split(get("EventOffer")), achievement_category=get("AchievementCategory") or "",
        )

    def challenge_text(r):
        """The challenge's objective (gui.window.dailychallenges_<Objective>Desc)
        with its amount ({0:i:NeededAmount}) and target ({0:s:MonsterName})
        filled in, as the daily card shows it."""
        raw = text(f"gui.window.dailychallenges_{r['Objective']}Desc", r["Id"])
        value = str(r["ObjectiveValue"] or "")
        amount, target = (value, "") if value.isdigit() else ("1", value)
        name = text(f"db.monster.table_{target}Name", text(f"db.npc.table_{target}Name", target.replace("_", " ").title()))
        raw = re.sub(r"\{\d+:i(:[^}]*)?\}", amount, raw)
        return re.sub(r"\{\d+:s(:[^}]*)?\}", name, raw).strip()

    challenges = {}
    for r in q("SELECT Id, LocaleId, Objective, ObjectiveValue, Reward, Category, Tier, ChallengeDifficulty, Image FROM _Template_DailyChallenges"):
        challenges[r["Id"]] = dict(
            description=challenge_text(r),
            hint=text(f"gui.window.dailychallenges_{r['LocaleId']}") if r["LocaleId"] else "",
            objective=r["Objective"] or "", value=int(r["ObjectiveValue"] or 0) if str(r["ObjectiveValue"] or "").isdigit() else 0,
            reward=r["Reward"] or "", category=r["Category"] or "", tier=int(r["Tier"] or 0), difficulty=r["ChallengeDifficulty"] or "",
            image=texture(r["Image"]),
        )

    wanted = {}
    for r in q("SELECT Id, QuestBrief, QuestId, Rarity, RewardShow, QuestDetails, BackgroundImage FROM _Template_WantedMissionAll"):
        wanted[str(r["Id"])] = dict(
            title=text(f"gui.window.wantedmissions_{r['QuestBrief']}", r["QuestBrief"] or ""),
            details=text(f"gui.window.wantedmissions_{r['QuestDetails']}") if r["QuestDetails"] else "",
            quest=r["QuestId"] or "", rarity=r["Rarity"] or "", reward=r["RewardShow"] or "", image=texture(r["BackgroundImage"]),
        )

    # The album (UI::StickerbookWidget): books by category and subcategory,
    # each with its rewards for the stickers collected ("count:item:amount");
    # a sticker's items, origin icon, preview background, offer, novelty; the
    # front page's banners (their English textures, by priority).
    def book_rewards(value):
        out = []
        for entry in str(value or "").split(";"):
            # "count:item:amount", or "count:status effect" (the pet books'
            # Collector's Bag bonuses: "3:sb_pets_increased_hitpoints_01").
            m = re.match(r"(\d+):([^:]+)(?::(\d+))?$", entry.strip())
            if m:
                out.append(dict(at=int(m.group(1)), item=m.group(2), amount=int(m.group(3) or 1)))
        return out
    books = [dict(id=r["Id"], category=int(r["StickerbookCategoryId"]), subcategory=int(r["StickerbookSubcategoryId"]),
                  priority=int(r["StickerbookPriority"] or 0), title=text(f"db.stickerbookcategory.table_{r['Id']}Title", r["Id"]),
                  description=text(f"db.stickerbookcategory.table_{r['Id']}Desc"), rewards=book_rewards(r["StickerbookRewardItems"]))
             for r in q("SELECT Id, StickerbookCategoryId, StickerbookSubcategoryId, StickerbookPriority, StickerbookRewardItems FROM _Template_Stickerbook ORDER BY StickerbookCategoryId, StickerbookSubcategoryId, StickerbookPriority, rowid")]
    stickers = {}
    # `order`: its row, the album's order within a book and priority (the
    # original's Tamed Foes grid: Molten Fire Guard, Jumpy Squire, Varnok...).
    for r in q("SELECT rowid AS RowId, Id, Name, StickerbookId, Items, RarityUpgradeItems, Priority, IconBrush, BackgroundTexture, StickerbookOffer, IsNew "
               "FROM _Template_StickerbookItem WHERE IsLive<>0 ORDER BY StickerbookId, Priority, rowid"):
        stickers[r["Id"]] = dict(name=plain(r["Name"] or r["Id"]), book=r["StickerbookId"] or "", items=split(r["Items"]),
                                 upgrades=split(r["RarityUpgradeItems"]), priority=int(r["Priority"] or 0), order=int(r["RowId"]),
                                 origin=texture(r["IconBrush"]), background=texture(r["BackgroundTexture"]),
                                 offer=r["StickerbookOffer"] or "", new=bool(r["IsNew"]))
    # What the album's preview turns for each of a sticker's items
    # (client/src/ui/preview.rs): the class it is for (CharClass; the original
    # keeps the player's class's, StickerbookWidget::FilterStickerbookItemsByPlayerClass),
    # the status effect it starts (StatusEffectId; its wire index is its rowid - 1)
    # and what that effect shows: a costume's skin part (SkinModifier), a mount
    # (the cast effect's ride, ActorStatusEffect in its DoneModifiers, whose
    # Mounted:Mount names it), a pet (Pet:PetMonster), a Shadow Soul's monster
    # (ShadowSoul), else an emote's animation state (StartAnimation). The
    # decorations (trails, ground markers, auras, backs) only start effects.
    statuses = {r["Id"]: r for r in q("SELECT rowid, Id, StartModifiers, DoneModifiers, StartAnimation FROM _Template_StatusEffect")}
    item_rows = {r["Id"]: r for r in q("SELECT Id, StatusEffectId, CharClass FROM _Template_Item")}

    def modifier(status, name):
        for m in str(status["StartModifiers"] or "").split(";"):
            m = m.split("#")[0].strip()
            if m.startswith(name + ":"):
                return m[len(name) + 1:].split(",")
        return []

    def sticker_look(item):
        r = item_rows.get(item)
        s = statuses.get(r["StatusEffectId"] or "") if r else None
        if s is None:
            return None
        out = dict(item=item, status=int(s["rowid"]) - 1)
        if r["CharClass"]:
            out["class"] = r["CharClass"]
        ride = re.search(r"ActorStatusEffect:addae,([^,;]+)", s["DoneModifiers"] or "")
        mount = modifier(statuses[ride.group(1)], "Mounted") if ride and ride.group(1) in statuses else []
        pet, skin, soul = modifier(s, "Pet"), modifier(s, "SkinModifier"), modifier(s, "ShadowSoul")
        if len(mount) > 1:
            out["mount"] = mount[1]
        elif len(pet) > 1 and pet[0] == "PetMonster":
            out["pet"] = pet[1]
        elif skin:
            out["skin"] = skin[0]
        elif soul:
            out["monster"] = soul[0]
        elif s["StartAnimation"]:
            out["animation"] = s["StartAnimation"]
        return out

    for sticker in stickers.values():
        sticker["looks"] = [look for look in map(sticker_look, sticker["items"]) if look]
    album_banners = [texture(f"{r['TexturePrefix']}en{r['TextureSuffix'] or ''}")
                     for r in q("SELECT TexturePrefix, TextureSuffix FROM _Template_StickerbookBanner WHERE IsActive<>0 ORDER BY Priority, rowid")]

    # The workbench (UI::RecipeBookWidget): tabs (WorkbenchTab, icon
    # ui/workbench_tab_<tab>), each a tree of categories, whose leaves list
    # recipe books (_Template_RecipeBook: title, description, ingredient
    # placeholders); a book is crafted by its recipes (RecipeBookReference).
    workbench_categories = [dict(id=r["Category"], tab=r["WorkbenchTab"] or "", parent=r["Parent"] or "", priority=int(r["Priority"] or 0),
                                 title=text(f"db.recipebookcategory.table_{r['Category']}", r["Category"]))
                            for r in q("SELECT Category, WorkbenchTab, Parent, Priority FROM _Template_RecipeBookCategory WHERE Enabled<>0 ORDER BY Priority, rowid")]
    tabs = []
    for c in workbench_categories:
        if c["tab"] and c["tab"] not in [t["id"] for t in tabs]:
            tabs.append(dict(id=c["tab"], icon=texture(f"ui/workbench_tab_{c['tab'].lower()}")))
    recipes = {}
    by_book = {}
    for r in q("SELECT Id, CraftedItemId, IngredientItemId1, IngredientItemId2, IngredientItemId3, IngredientItemId4, PriceVC, PriceRC, "
               "CraftingCategory, UnlockLevel, RecipeBookReference, RecipeItemReference FROM _Template_CraftingRecipes WHERE IsLive<>0 AND RecipeBookReference IS NOT NULL AND RecipeBookReference!=''"):
        recipes[r["Id"]] = dict(
            crafted=r["CraftedItemId"] or "", ingredients=[r[f"IngredientItemId{n}"] for n in range(1, 5) if r[f"IngredientItemId{n}"]],
            price_vc=int(r["PriceVC"] or 0), price_rc=int(r["PriceRC"] or 0), category=r["CraftingCategory"] or "",
            level=int(r["UnlockLevel"] or 0), book=r["RecipeBookReference"], unlock=r["RecipeItemReference"] or "",
        )
        by_book.setdefault(r["RecipeBookReference"], []).append(r["Id"])
    recipe_books = {}
    for r in q("SELECT * FROM _Template_RecipeBook"):
        if r["Id"] not in by_book:
            continue
        placeholders = []
        for n in range(1, 6):
            icon = r[f"Placeholder{n}IconBrush"]
            title = r[f"Placeholder{n}Title"]
            if icon or title:
                placeholders.append(dict(icon=texture(icon), title=text(f"db.recipebook.table_{title}Title", title or ""), rarity=r[f"Placeholder{n}DefaultRarity"] or ""))
        recipe_books[r["Id"]] = dict(
            category=r["RecipeCategory"] or "", priority=int(r["Priority"] or 0),
            title=text(f"db.recipebook.table_{r['Id']}Title"), description=text(f"db.recipebook.table_{r['RecipeDesc']}Desc") if r["RecipeDesc"] else "",
            locked=text(f"db.recipebook.table_{r['LockedDesc']}Desc", text(f"db.recipebook.table_{r['LockedDesc']}")) if r["LockedDesc"] else "",
            unlockable=bool(r["ShowAsUnlockable"]), placeholders=placeholders, recipes=by_book[r["Id"]],
            locked_icon=texture(r["LockedIcon"]) if "LockedIcon" in r.keys() else "",
        )

    # The shops (UI::ShopWidget): a tab per shop, each a category tree whose
    # leaves filter the shop's offers by OfferCategory; "$$$" is the premium
    # (real money) shop, global_rc_trader the Andermant item shop.
    shop_categories = [dict(shop=r["ShopId"], id=r["Category"], parent=r["Parent"] or "", priority=int(r["Priority"] or 0),
                            offers=split(r["OfferCategory"]), tabs=split(r["TabCategory"]),
                            title=text(f"db.shopcategory.table_{r['Category']}Title", r["Category"]))
                       for r in q("SELECT * FROM _Template_ShopCategory WHERE IsActive<>0 ORDER BY Priority, rowid")]
    for ref in ("ui/icon_tab_ocp_andermant", "ui/icon_tab_cash", "ui/icon_tab_early", "icons/money_rc_pile"):
        texture(ref)
    shop_banners = []
    for r in q("SELECT * FROM _Template_ShopBanners WHERE Enabled<>0 ORDER BY Priority, rowid"):
        ref = f"{r['TexturePrefix']}en{r['TextureSuffix']}"
        shop_banners.append(dict(texture=texture(ref), category=r["ReferenceShopCategory"] or "", offer=r["ReferenceOfferId"] or ""))

    # A skin fits the equipment of its slot type (the inventory slots of
    # inventory_catalog.SLOTS) worn by its classes; the server charges ApplyPrice.
    from inventory_catalog import SLOTS
    # The transmog desk (UI::CustomizationWidget): the skins (the slots they
    # fit, their classes, prices, the removal's, PvE or PvP, whether they carry
    # an effect or a shop offer, the item that shows them), the dyes (the dye
    # item that pays for one, prices) and the gold prices' factor by level
    # (_Template_DynamicLevelCosts, item_customization_protocol).
    skins = {}
    for r in q("SELECT Id, LocaleIdTitle, SlotType, CharClass, IconBrush, ApplyPriceRC, ApplyPriceVC, RemovePrice, EquipmentType, "
               "EffectSequence, OfferId, TooltipItem, Priority FROM _Template_EquipmentSkin WHERE Visible=1"):
        skins[r["Id"]] = dict(title=text(f"db.equipmentskin.table_{r['LocaleIdTitle']}") if r["LocaleIdTitle"] else "",
                              slots=SLOTS.get(r["SlotType"] or "", []), classes=split(r["CharClass"]), icon=texture(r["IconBrush"]),
                              price_rc=int(r["ApplyPriceRC"] or 0), price_vc=int(r["ApplyPriceVC"] or 0), remove_vc=int(r["RemovePrice"] or 0),
                              kind=r["EquipmentType"] or "PvE", effect=bool(r["EffectSequence"]), offer=r["OfferId"] or "",
                              item=r["TooltipItem"] or "", priority=int(r["Priority"] or 0))
    dyes = {r["Id"]: dict(item=r["Color"] or "", icon=texture(r["IconBrush"]), price_rc=int(r["ApplyPriceRC"] or 0),
                          price_vc=int(r["ApplyPriceVC"] or 0), remove_vc=int(r["RemovePrice"] or 0), kind=r["EquipmentType"] or "PvE",
                          priority=int(r["Priority"] or 0))
            for r in q("SELECT * FROM _Template_EquipmentColor WHERE Visible=1")}
    level_costs = [float(r[0]) for r in q("SELECT VirtualCurrencyCostMultiplier FROM _Template_DynamicLevelCosts ORDER BY Level")]

    sockets = {r["Id"]: {k: int(r[k] or 0) for k in ("Common", "Uncommon", "Magic", "Rare", "Epic", "Unique", "Mythic", "Set", "TrinketSocketCosts", "Amount")}
               for r in q("SELECT * FROM _Template_SocketExtensionCosts")}

    # The currency bag's rows after the Glyphs of Power (UI::CurrencyBagWidget):
    # each currency item of the shops once, with its icon, by Priority, highest
    # first, ties in table order.
    # EVIDENCE: release261 Managers::TemplateManager::LoadItemCurrency
    #   (0x140866A94) keeps one entry a CurrencyItem (the highest Priority) in
    #   table order; std::sort with 0x140A2ECC0 (a before b when a.Priority >
    #   b.Priority) orders them -- an insertion sort under 33 entries, so ties
    #   keep their order.
    priorities, currency_icons = {}, {}
    for r in q("SELECT CurrencyItem, CurrencyIcon, Priority FROM _Template_ItemCurrency ORDER BY rowid"):
        item = r["CurrencyItem"] or ""
        if not item:
            continue
        if item not in priorities:
            currency_icons[item] = texture(r["CurrencyIcon"])
        priorities[item] = max(priorities.get(item, -2**31), int(r["Priority"] or 0))
    currencies = [dict(id=item, icon=currency_icons[item]) for item in sorted(priorities, key=lambda i: -priorities[i])]

    # PvP ranks (the titles of the honour levels).
    pvp_ranks = [text(f"db.honorlevels.title_pvptitle{n:02d}") for n in range(0, 40) if en.get(f"db.honorlevels.title_pvptitle{n:02d}")]

    # The PvP queue's modes (the matchmaking window's tabs: team, solo), with
    # their artwork and rules (pvp_match_protocol MATCH_MODE_* by battleground).
    def rules(key, a, b):
        # {2:i} is the match's duration: the battlegrounds' 10 minutes.
        return re.sub(r"\{2:i[^}]*\}", "10", re.sub(r"\{1:i[^}]*\}", str(b), re.sub(r"\{0:i[^}]*\}", str(a), text(key))))
    pvp_modes = []
    for mode, battleground, title, description, sizes in (
            (1, "BestOf5_5v5_01", "gui.window.pvp_BestOfX", "gui.window.pvp_descriptionBestOfX", (5, 5)),
            (0, "BestOf3_1v1_01", "gui.window.pvp_Duel", "gui.window.pvp_descriptionDuel", (1, 1))):
        row = db.execute("SELECT Artwork FROM _Template_BattlegroundSettings WHERE Id=?", (battleground,)).fetchone()
        pvp_modes.append(dict(mode=mode, title=text(title), artwork=texture(row["Artwork"] if row else ""),
                              description=rules(description, *sizes), reward=text("gui.window.pvp_descriptionReward")))
    # The overall PvP board's rewards by place.
    pvp_rewards = [dict(first=int(r["PlaceFrom"]), last=int(r["PlaceTo"]), items=[i.split(":")[0] for i in split(r["Items"])])
                   for r in q("SELECT * FROM _Template_OverallPVPRankingRewards ORDER BY PlaceFrom")]

    # The offers' category codes by name (shop_protocol.OFFER_CATEGORY_IDS,
    # Game::OfferCategory): the shops' category trees name them.
    offer_categories = {name: code for code, name in enumerate((
        "Potion", "Elixir", "Tonic", "BuffItem", "ItemGem", "OneHand", "TwoHand", "Armor", "Shield", "Jewelry", "Glyph",
        "Essence", "Color", "Mount", "Pakete", "Pet", "Event", "GamblerBarter", "Privileged", "PrivilegedDaily", "Cloak",
        "OneUseEmote", "Emote", "TempCostume", "Costume", "RCSurpriseBoxes", "ECSurpriseBoxes", "ECOffers", "LimitedOffers",
        "TransferItem", "ItemGemCe55", "RecipeVC", "RecipeRC", "RecipeDraken", "RecipeMateriFragment", "RecipeHonor",
        "IngredientVC", "IngredientRC", "IngredientDraken", "IngredientMateriFragment", "IngredientHonor", "TravelItem",
        "TravelItemEvent", "TravelItemParallelWorld", "SkinConsumable", "SeasonPassItem", "Misc", "QA"))}
    offer_categories["Key"] = offer_categories["Misc"]

    # The daily and Wanted figures: the reshuffle's and refreshes' Andermant,
    # the Wanted missions a day (base, privileged).
    g = db.execute("SELECT DailyAchievementReshuffleCosts, WantedBaseTimes, WantedVIPTimes, WantedSingleRefreshRC, "
                   "WantedAllRefreshRC, WantedVIPBuyRC FROM _Globals LIMIT 1").fetchone()
    figures = dict(zip(("daily_reshuffle", "wanted_base", "wanted_vip", "wanted_single_refresh", "wanted_all_refresh", "wanted_vip_buy"),
                       (int(v or 0) for v in g))) if g else {}

    out = os.path.join(assets, "interface", "systems.catalog.json")
    with open(out, "w", encoding="utf-8") as f:
        json.dump(dict(achievements=achievements, achievement_categories=categories, events=events, challenges=challenges, wanted=wanted,
                       stickerbooks=books, stickers=stickers, recipes=recipes, recipe_books=recipe_books, workbench_tabs=tabs,
                       workbench_categories=workbench_categories, shop_categories=shop_categories, shop_banners=shop_banners,
                       skins=skins, socket_costs=sockets, pvp_ranks=pvp_ranks, pvp_modes=pvp_modes, pvp_rewards=pvp_rewards,
                       offer_categories=offer_categories, figures=figures, album_banners=album_banners, dyes=dyes, level_costs=level_costs,
                       currencies=currencies),
                  f, ensure_ascii=False, separators=(",", ":"))

    # The runtime artwork, converted once (tools/export_ui.py's converter).
    converted = missing = 0
    crunch = os.path.abspath(os.path.join(godot, "build", "crn2dds.exe"))
    if os.name == "nt" and os.path.isfile(crunch):
        os.environ.setdefault("DSO_CRN2DDS", crunch)
    from dsoexp.resources import Resolver
    convert = gx.Textures(Resolver(root), os.path.join(assets, "textures"), "textures")
    for ref in sorted(textures):
        if os.path.isfile(os.path.join(assets, "textures", ref + ".png")):
            continue
        if convert.get(ref):
            converted += 1
        else:
            missing += 1
    return dict(achievements=len(achievements), events=len(events), challenges=len(challenges), wanted=len(wanted),
                books=len(books), stickers=len(stickers), recipes=len(recipes), recipe_books=len(recipe_books),
                shop_categories=len(shop_categories), skins=len(skins), currencies=len(currencies),
                textures_converted=converted, textures_missing=missing)


if __name__ == "__main__":
    print("systems:", export_systems(*sys.argv[1:5]))
