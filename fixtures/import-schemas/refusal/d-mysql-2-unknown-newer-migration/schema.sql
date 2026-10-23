
/*!40101 SET @OLD_CHARACTER_SET_CLIENT=@@CHARACTER_SET_CLIENT */;
/*!40101 SET @OLD_CHARACTER_SET_RESULTS=@@CHARACTER_SET_RESULTS */;
/*!40101 SET @OLD_COLLATION_CONNECTION=@@COLLATION_CONNECTION */;
/*!50503 SET NAMES utf8mb4 */;
/*!40103 SET @OLD_TIME_ZONE=@@TIME_ZONE */;
/*!40103 SET TIME_ZONE='+00:00' */;
/*!40014 SET @OLD_UNIQUE_CHECKS=@@UNIQUE_CHECKS, UNIQUE_CHECKS=0 */;
/*!40014 SET @OLD_FOREIGN_KEY_CHECKS=@@FOREIGN_KEY_CHECKS, FOREIGN_KEY_CHECKS=0 */;
/*!40101 SET @OLD_SQL_MODE=@@SQL_MODE, SQL_MODE='NO_AUTO_VALUE_ON_ZERO' */;
/*!40111 SET @OLD_SQL_NOTES=@@SQL_NOTES, SQL_NOTES=0 */;
DROP TABLE IF EXISTS `_prisma_migrations`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `_prisma_migrations` (
  `id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `checksum` varchar(64) COLLATE utf8mb4_unicode_ci NOT NULL,
  `finished_at` datetime(3) DEFAULT NULL,
  `migration_name` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `logs` text COLLATE utf8mb4_unicode_ci,
  `rolled_back_at` datetime(3) DEFAULT NULL,
  `started_at` datetime(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  `applied_steps_count` int unsigned NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
DROP TABLE IF EXISTS `event_data`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `event_data` (
  `event_data_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `website_event_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `website_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `data_key` varchar(500) COLLATE utf8mb4_unicode_ci NOT NULL,
  `string_value` varchar(500) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `number_value` decimal(19,4) DEFAULT NULL,
  `date_value` timestamp NULL DEFAULT NULL,
  `data_type` int unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (`event_data_id`),
  KEY `event_data_created_at_idx` (`created_at`),
  KEY `event_data_website_id_idx` (`website_id`),
  KEY `event_data_website_event_id_idx` (`website_event_id`),
  KEY `event_data_website_id_created_at_idx` (`website_id`,`created_at`),
  KEY `event_data_website_id_created_at_data_key_idx` (`website_id`,`created_at`,`data_key`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
DROP TABLE IF EXISTS `report`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `report` (
  `report_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `user_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `website_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `type` varchar(200) COLLATE utf8mb4_unicode_ci NOT NULL,
  `name` varchar(200) COLLATE utf8mb4_unicode_ci NOT NULL,
  `description` varchar(500) COLLATE utf8mb4_unicode_ci NOT NULL,
  `parameters` json NOT NULL,
  `created_at` timestamp NULL DEFAULT CURRENT_TIMESTAMP,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`report_id`),
  UNIQUE KEY `report_report_id_key` (`report_id`),
  KEY `report_user_id_idx` (`user_id`),
  KEY `report_website_id_idx` (`website_id`),
  KEY `report_type_idx` (`type`),
  KEY `report_name_idx` (`name`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
DROP TABLE IF EXISTS `revenue`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `revenue` (
  `revenue_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `website_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `session_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `event_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `event_name` varchar(50) COLLATE utf8mb4_unicode_ci NOT NULL,
  `currency` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL,
  `revenue` decimal(19,4) DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (`revenue_id`),
  UNIQUE KEY `revenue_revenue_id_key` (`revenue_id`),
  KEY `revenue_website_id_idx` (`website_id`),
  KEY `revenue_session_id_idx` (`session_id`),
  KEY `revenue_website_id_created_at_idx` (`website_id`,`created_at`),
  KEY `revenue_website_id_session_id_created_at_idx` (`website_id`,`session_id`,`created_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
DROP TABLE IF EXISTS `segment`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `segment` (
  `segment_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `website_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `type` varchar(200) COLLATE utf8mb4_unicode_ci NOT NULL,
  `name` varchar(200) COLLATE utf8mb4_unicode_ci NOT NULL,
  `parameters` json NOT NULL,
  `created_at` timestamp NULL DEFAULT CURRENT_TIMESTAMP,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`segment_id`),
  UNIQUE KEY `segment_segment_id_key` (`segment_id`),
  KEY `segment_website_id_idx` (`website_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
DROP TABLE IF EXISTS `session`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `session` (
  `session_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `website_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `browser` varchar(20) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `os` varchar(20) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `device` varchar(20) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `screen` varchar(11) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `language` varchar(35) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `country` char(2) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `region` char(20) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `city` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT CURRENT_TIMESTAMP,
  `distinct_id` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  PRIMARY KEY (`session_id`),
  UNIQUE KEY `session_session_id_key` (`session_id`),
  KEY `session_created_at_idx` (`created_at`),
  KEY `session_website_id_idx` (`website_id`),
  KEY `session_website_id_created_at_idx` (`website_id`,`created_at`),
  KEY `session_website_id_created_at_browser_idx` (`website_id`,`created_at`,`browser`),
  KEY `session_website_id_created_at_os_idx` (`website_id`,`created_at`,`os`),
  KEY `session_website_id_created_at_device_idx` (`website_id`,`created_at`,`device`),
  KEY `session_website_id_created_at_screen_idx` (`website_id`,`created_at`,`screen`),
  KEY `session_website_id_created_at_language_idx` (`website_id`,`created_at`,`language`),
  KEY `session_website_id_created_at_country_idx` (`website_id`,`created_at`,`country`),
  KEY `session_website_id_created_at_city_idx` (`website_id`,`created_at`,`city`),
  KEY `session_website_id_created_at_region_idx` (`website_id`,`created_at`,`region`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
DROP TABLE IF EXISTS `session_data`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `session_data` (
  `session_data_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `website_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `session_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `data_key` varchar(500) COLLATE utf8mb4_unicode_ci NOT NULL,
  `string_value` varchar(500) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `number_value` decimal(19,4) DEFAULT NULL,
  `date_value` timestamp NULL DEFAULT NULL,
  `data_type` int unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT CURRENT_TIMESTAMP,
  `distinct_id` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  PRIMARY KEY (`session_data_id`),
  KEY `session_data_created_at_idx` (`created_at`),
  KEY `session_data_website_id_idx` (`website_id`),
  KEY `session_data_session_id_idx` (`session_id`),
  KEY `session_data_session_id_created_at_idx` (`session_id`,`created_at`),
  KEY `session_data_website_id_created_at_data_key_idx` (`website_id`,`created_at`,`data_key`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
DROP TABLE IF EXISTS `team`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `team` (
  `team_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `name` varchar(50) COLLATE utf8mb4_unicode_ci NOT NULL,
  `access_code` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT CURRENT_TIMESTAMP,
  `updated_at` timestamp NULL DEFAULT NULL,
  `deleted_at` timestamp NULL DEFAULT NULL,
  `logo_url` varchar(2183) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  PRIMARY KEY (`team_id`),
  UNIQUE KEY `team_team_id_key` (`team_id`),
  UNIQUE KEY `team_access_code_key` (`access_code`),
  KEY `team_access_code_idx` (`access_code`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
DROP TABLE IF EXISTS `team_user`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `team_user` (
  `team_user_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `team_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `user_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `role` varchar(50) COLLATE utf8mb4_unicode_ci NOT NULL,
  `created_at` timestamp NULL DEFAULT CURRENT_TIMESTAMP,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`team_user_id`),
  UNIQUE KEY `team_user_team_user_id_key` (`team_user_id`),
  KEY `team_user_team_id_idx` (`team_id`),
  KEY `team_user_user_id_idx` (`user_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
DROP TABLE IF EXISTS `user`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `user` (
  `user_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `username` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `password` varchar(60) COLLATE utf8mb4_unicode_ci NOT NULL,
  `role` varchar(50) COLLATE utf8mb4_unicode_ci NOT NULL,
  `created_at` timestamp NULL DEFAULT CURRENT_TIMESTAMP,
  `updated_at` timestamp NULL DEFAULT NULL,
  `deleted_at` timestamp NULL DEFAULT NULL,
  `display_name` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `logo_url` varchar(2183) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  PRIMARY KEY (`user_id`),
  UNIQUE KEY `user_user_id_key` (`user_id`),
  UNIQUE KEY `user_username_key` (`username`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
DROP TABLE IF EXISTS `website`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `website` (
  `website_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `name` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL,
  `domain` varchar(500) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `share_id` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `reset_at` timestamp NULL DEFAULT NULL,
  `user_id` varchar(36) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT CURRENT_TIMESTAMP,
  `updated_at` timestamp NULL DEFAULT NULL,
  `deleted_at` timestamp NULL DEFAULT NULL,
  `created_by` varchar(36) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `team_id` varchar(36) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `future_flag` tinyint(1) NOT NULL DEFAULT '0',
  PRIMARY KEY (`website_id`),
  UNIQUE KEY `website_website_id_key` (`website_id`),
  UNIQUE KEY `website_share_id_key` (`share_id`),
  KEY `website_user_id_idx` (`user_id`),
  KEY `website_created_at_idx` (`created_at`),
  KEY `website_share_id_idx` (`share_id`),
  KEY `website_team_id_idx` (`team_id`),
  KEY `website_created_by_idx` (`created_by`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
DROP TABLE IF EXISTS `website_event`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `website_event` (
  `event_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `website_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `session_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `created_at` timestamp NULL DEFAULT CURRENT_TIMESTAMP,
  `url_path` varchar(500) COLLATE utf8mb4_unicode_ci NOT NULL,
  `url_query` varchar(500) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `referrer_path` varchar(500) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `referrer_query` varchar(500) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `referrer_domain` varchar(500) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `page_title` varchar(500) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `event_type` int unsigned NOT NULL DEFAULT '1',
  `event_name` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `visit_id` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `tag` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `fbclid` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `gclid` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `li_fat_id` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `msclkid` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `ttclid` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `twclid` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `utm_campaign` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `utm_content` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `utm_medium` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `utm_source` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `utm_term` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `hostname` varchar(100) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  PRIMARY KEY (`event_id`),
  KEY `website_event_created_at_idx` (`created_at`),
  KEY `website_event_session_id_idx` (`session_id`),
  KEY `website_event_website_id_idx` (`website_id`),
  KEY `website_event_website_id_created_at_idx` (`website_id`,`created_at`),
  KEY `website_event_website_id_session_id_created_at_idx` (`website_id`,`session_id`,`created_at`),
  KEY `website_event_website_id_created_at_url_path_idx` (`website_id`,`created_at`,`url_path`),
  KEY `website_event_website_id_created_at_url_query_idx` (`website_id`,`created_at`,`url_query`),
  KEY `website_event_website_id_created_at_referrer_domain_idx` (`website_id`,`created_at`,`referrer_domain`),
  KEY `website_event_website_id_created_at_page_title_idx` (`website_id`,`created_at`,`page_title`),
  KEY `website_event_website_id_created_at_event_name_idx` (`website_id`,`created_at`,`event_name`),
  KEY `website_event_visit_id_idx` (`visit_id`),
  KEY `website_event_website_id_visit_id_created_at_idx` (`website_id`,`visit_id`,`created_at`),
  KEY `website_event_website_id_created_at_tag_idx` (`website_id`,`created_at`,`tag`),
  KEY `website_event_website_id_created_at_hostname_idx` (`website_id`,`created_at`,`hostname`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
/*!40103 SET TIME_ZONE=@OLD_TIME_ZONE */;

/*!40101 SET SQL_MODE=@OLD_SQL_MODE */;
/*!40014 SET FOREIGN_KEY_CHECKS=@OLD_FOREIGN_KEY_CHECKS */;
/*!40014 SET UNIQUE_CHECKS=@OLD_UNIQUE_CHECKS */;
/*!40101 SET CHARACTER_SET_CLIENT=@OLD_CHARACTER_SET_CLIENT */;
/*!40101 SET CHARACTER_SET_RESULTS=@OLD_CHARACTER_SET_RESULTS */;
/*!40101 SET COLLATION_CONNECTION=@OLD_COLLATION_CONNECTION */;
/*!40111 SET SQL_NOTES=@OLD_SQL_NOTES */;

